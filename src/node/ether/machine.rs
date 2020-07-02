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
    use crate::node;
    use crate::priv_prelude::*;
    use crate::spawn;
    use rand;
    use std;
    use tokio::net::UdpSocket;
    use void;

    #[test]
    fn one_interface_send_udp_ipv4() {
        run_test(3, || {
            let mut runtime = unwrap!(Runtime::new());
            let network = Network::new();
            let handle = network.handle();
            let res = runtime.block_on(future::lazy(move || {
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
                    node::ether::machine(move |_mac_addr, ipv4_addr_opt, _ipv6_addr_opt| {
                        let ipv4_addr = unwrap!(ipv4_addr_opt);
                        unwrap!(ipv4_addr_tx.send(ipv4_addr));

                        let socket = unwrap!(UdpSocket::bind(&addr!("0.0.0.0:0")));
                        socket
                            .send_dgram(payload, &SocketAddr::V4(target_addr))
                            .map_err(|e| panic!("error sending: {}", e))
                            .map(|(_socket, _payload)| {
                                trace!("sent udp packet");
                            })
                    }),
                );
                let (tx, rx) = plug.split();

                let iface_ip = unwrap!(ipv4_addr_rx.recv());

                let gateway_mac = MacAddr::random();

                rx.into_future()
                    .map_err(|(v, _rx)| void::unreachable(v))
                    .and_then(move |(frame_opt, rx)| {
                        let frame = unwrap!(frame_opt);
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

                        tx.send(frame)
                            .map_err(|_e| panic!("channel hung up!"))
                            .and_then(|_tx| {
                                rx.into_future().map_err(|(v, _rx)| void::unreachable(v))
                            })
                            .and_then(move |(frame_opt, _rx)| {
                                let frame = unwrap!(frame_opt);
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

                                spawn_complete.map_err(|e| panic::resume_unwind(e))
                            })
                    })
            }));
            res.void_unwrap()
        })
    }
}
