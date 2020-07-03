use crate::device::MachineBuilder;
use crate::iface::EtherIfaceBuilder;
use crate::network::NetworkHandle;
use crate::node::ether::EtherNode;
use crate::range::{Ipv4Range, Ipv6Range};
use crate::route::{Ipv4Route, Ipv6Route};
use crate::spawn_complete::SpawnComplete;
use crate::wire::{EtherPlug, MacAddr};
use async_std::net::{Ipv4Addr, Ipv6Addr};
use futures::future::Future;

/// A node representing an ethernet machine
pub struct MachineNode<F> {
    func: F,
}

/// Create a node for a machine with an ethernet interface. This node will run the given function
/// in a network namespace with a single interface.
pub fn machine<T, F>(func: F) -> MachineNode<F>
where
    T: Future + Send + 'static,
    T::Output: Unpin + Send + 'static,
    F: FnOnce(MacAddr, Option<Ipv4Addr>, Option<Ipv6Addr>) -> T + Send + 'static,
{
    MachineNode { func }
}

impl<T, F> EtherNode for MachineNode<F>
where
    T: Future + Send + 'static,
    T::Output: Unpin + Send + 'static,
    F: FnOnce(MacAddr, Option<Ipv4Addr>, Option<Ipv6Addr>) -> T + Send + 'static,
{
    type Output = T::Output;

    fn build(
        self,
        handle: &NetworkHandle,
        ipv4_range: Option<Ipv4Range>,
        ipv6_range: Option<Ipv6Range>,
    ) -> (SpawnComplete<Self::Output>, EtherPlug) {
        let mac_addr = MacAddr::random();
        let mut iface = { EtherIfaceBuilder::new().mac_addr(mac_addr) };
        let ipv4_addr = match ipv4_range {
            Some(range) => {
                let address = range.random_client_addr();
                iface = {
                    iface
                        .ipv4_addr(address, range.netmask_prefix_length())
                        .ipv4_route(Ipv4Route::new(
                            Ipv4Range::global(),
                            Some(range.gateway_ip()),
                        ))
                };
                Some(address)
            }
            None => None,
        };
        let ipv6_addr = match ipv6_range {
            Some(range) => {
                let address = range.random_client_addr();
                iface = {
                    iface
                        .ipv6_addr(address, range.netmask_prefix_length())
                        .ipv6_route(Ipv6Route::new(Ipv6Range::global(), range.next_hop_ip()))
                };
                Some(address)
            }
            None => None,
        };
        let (plug_a, plug_b) = EtherPlug::new_pair();

        let spawn_complete = {
            MachineBuilder::new()
                .add_ether_iface(iface, plug_b)
                .spawn(handle, move || (self.func)(mac_addr, ipv4_addr, ipv6_addr))
        };

        (spawn_complete, plug_a)
    }
}

#[cfg(feature = "linux_host")]
#[cfg(test)]
mod test {
    use crate::network::Network;
    use crate::node;
    use crate::range::Ipv4Range;
    use crate::spawn;
    use crate::test::run_test;
    use crate::util::ipv4_addr::Ipv4AddrExt;
    use crate::wire::{
        ArpFields, EtherFields, EtherFrame, EtherPayload, EtherPayloadFields, Ipv4Payload, MacAddr,
    };
    use async_std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
    use async_std::task;
    use futures::sink::SinkExt;
    use futures::stream::StreamExt;

    #[test]
    fn one_interface_send_udp_ipv4() {
        run_test(3, || {
            let network = Network::new();
            let handle = network.handle();
            task::block_on(async move {
                trace!("starting");
                let payload: [u8; 8] = rand::random();
                let target_ip = Ipv4Addr::random_global();
                let target_port = rand::random::<u16>() / 2 + 1000;
                let target_addr = SocketAddrV4::new(target_ip, target_port);

                let range = Ipv4Range::random_local_subnet();
                let gateway_ip = range.gateway_ip();

                let (ipv4_addr_tx, ipv4_addr_rx) = std::sync::mpsc::channel();
                let (spawn_complete, plug) = spawn::ether_tree(
                    &handle,
                    Some(range),
                    None,
                    node::ether::machine(
                        move |_mac_addr, ipv4_addr_opt, _ipv6_addr_opt| async move {
                            let ipv4_addr = ipv4_addr_opt.unwrap();
                            ipv4_addr_tx.send(ipv4_addr).unwrap();

                            let socket = UdpSocket::bind(&addr!("0.0.0.0:0")).await.unwrap();
                            socket
                                .send_to(&payload, &SocketAddr::V4(target_addr))
                                .await
                                .unwrap();
                            trace!("sent udp packet");
                        },
                    ),
                );
                let (mut tx, mut rx) = plug.split();

                let iface_ip = ipv4_addr_rx.recv().unwrap();

                let gateway_mac = MacAddr::random();

                let frame = rx.next().await.unwrap();
                trace!("got frame from iface: {:?}", frame);
                let iface_mac = frame.source_mac();
                let arp = match frame.payload() {
                    EtherPayload::Arp(arp) => arp,
                    payload => panic!("unexpected payload: {:?}", payload),
                };
                assert_eq!(
                    arp.fields(),
                    ArpFields::Request {
                        source_mac: iface_mac,
                        source_ip: iface_ip,
                        dest_ip: gateway_ip,
                    }
                );
                let frame = EtherFrame::new_from_fields_recursive(
                    EtherFields {
                        source_mac: gateway_mac,
                        dest_mac: iface_mac,
                    },
                    EtherPayloadFields::Arp {
                        fields: ArpFields::Response {
                            source_mac: gateway_mac,
                            source_ip: gateway_ip,
                            dest_mac: iface_mac,
                            dest_ip: iface_ip,
                        },
                    },
                );

                tx.send(frame).await.unwrap();
                let frame = rx.next().await.unwrap();
                assert_eq!(
                    frame.fields(),
                    EtherFields {
                        source_mac: iface_mac,
                        dest_mac: gateway_mac,
                    }
                );
                let ipv4 = match frame.payload() {
                    EtherPayload::Ipv4(ipv4) => ipv4,
                    payload => panic!("unexpected payload: {:?}", payload),
                };
                assert_eq!(ipv4.source_ip(), iface_ip);
                assert_eq!(ipv4.dest_ip(), target_ip);
                let udp = match ipv4.payload() {
                    Ipv4Payload::Udp(udp) => udp,
                    payload => panic!("unexpected payload: {:?}", payload),
                };
                assert_eq!(udp.dest_port(), target_port);
                assert_eq!(&udp.payload(), &payload[..]);

                spawn_complete.await.unwrap();
            })
        })
    }
}
