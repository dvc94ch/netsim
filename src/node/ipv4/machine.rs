use super::Ipv4Node;
use crate::device::MachineBuilder;
use crate::iface::IpIfaceBuilder;
use crate::network::NetworkHandle;
use crate::range::Ipv4Range;
use crate::route::Ipv4Route;
use crate::spawn_complete::SpawnComplete;
use crate::wire::{IpPlug, Ipv4Plug};
use async_std::net::Ipv4Addr;
use futures::future::Future;

/// A node representing an Ipv4 machine.
pub struct MachineNode<F> {
    func: F,
}

/// Create a node for an Ipv4 machine. This node will run the given function in a network
/// namespace with a single interface in a separate thread of it's own.
pub fn machine<T, F>(func: F) -> MachineNode<F>
where
    T: Future + Send + 'static,
    T::Output: Unpin + Send + 'static,
    F: FnOnce(Ipv4Addr) -> T + Send + 'static,
{
    MachineNode { func }
}

impl<T, F> Ipv4Node for MachineNode<F>
where
    T: Future + Send + 'static,
    T::Output: Unpin + Send + 'static,
    F: FnOnce(Ipv4Addr) -> T + Send + 'static,
{
    type Output = T::Output;

    fn build(
        self,
        handle: &NetworkHandle,
        ipv4_range: Ipv4Range,
    ) -> (SpawnComplete<T::Output>, Ipv4Plug) {
        let address = ipv4_range.random_client_addr();
        let iface = {
            IpIfaceBuilder::new()
                .ipv4_addr(address, ipv4_range.netmask_prefix_length())
                .ipv4_route(Ipv4Route::new(Ipv4Range::global(), None))
        };
        let (plug_a, plug_b) = IpPlug::new_pair();

        let spawn_complete = {
            MachineBuilder::new()
                .add_ip_iface(iface, plug_b)
                .spawn(handle, move || (self.func)(address))
        };

        let plug_a = plug_a.into_ipv4_plug(handle);

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
        Icmpv4PacketKind, Ipv4Fields, Ipv4Packet, Ipv4Payload, Ipv4PayloadFields, TcpFields,
        UdpFields,
    };
    use async_std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket};
    use async_std::task;
    use bytes::Bytes;
    use futures::channel::oneshot;
    use futures::io::{AsyncReadExt, AsyncWriteExt};
    use futures::sink::SinkExt;
    use futures::stream::StreamExt;

    #[test]
    fn test_udp() {
        run_test(3, || {
            let network = Network::new();
            let handle = network.handle();

            task::block_on(async {
                let remote_ip = Ipv4Addr::random_global();
                let remote_port = rand::random::<u16>() / 2 + 1000;
                let remote_addr = SocketAddrV4::new(remote_ip, remote_port);

                let ipv4_range = Ipv4Range::random_local_subnet();
                let (ipv4_addr_tx, ipv4_addr_rx) = std::sync::mpsc::channel();
                let (spawn_complete, ipv4_plug) = spawn::ipv4_tree(
                    &handle,
                    ipv4_range,
                    node::ipv4::machine(move |ipv4_addr| async move {
                        ipv4_addr_tx.send(ipv4_addr).unwrap();
                        let buffer_out = rand::random::<[u8; 8]>();
                        let socket = UdpSocket::bind(&addr!("0.0.0.0:0")).await.unwrap();

                        socket
                            .send_to(&buffer_out, &SocketAddr::V4(remote_addr))
                            .await
                            .unwrap();
                        trace!("waiting to receive reply");
                        let mut buffer_in = [0u8; 8];
                        let (len, addr) = socket.recv_from(&mut buffer_in).await.unwrap();
                        trace!("received reply");
                        assert_eq!(len, 8);
                        assert_eq!(addr, SocketAddr::V4(remote_addr));
                        assert_eq!(buffer_out, buffer_in);
                    }),
                );

                let (mut plug_tx, mut plug_rx) = ipv4_plug.split();
                let iface_ip = ipv4_addr_rx.recv().unwrap();

                let packet = plug_rx.next().await.unwrap();
                assert_eq!(packet.source_ip(), iface_ip);
                assert_eq!(packet.dest_ip(), remote_ip);

                let udp = match packet.payload() {
                    Ipv4Payload::Udp(udp) => udp,
                    payload => panic!("unexpected packet payload: {:?}", payload),
                };
                assert_eq!(udp.dest_port(), remote_port);
                let iface_port = udp.source_port();

                let reply_packet = Ipv4Packet::new_from_fields_recursive(
                    Ipv4Fields {
                        source_ip: remote_ip,
                        dest_ip: iface_ip,
                        ttl: 12,
                    },
                    Ipv4PayloadFields::Udp {
                        fields: UdpFields {
                            source_port: remote_addr.port(),
                            dest_port: iface_port,
                        },
                        payload: udp.payload(),
                    },
                );

                trace!("sending reply packet");
                plug_tx.send(reply_packet).await.unwrap();
                spawn_complete.await.unwrap();
            })
        })
    }

    #[test]
    fn test_tcp_connect() {
        run_test(3, || {
            let network = Network::new();
            let handle = network.handle();

            task::block_on(async move {
                let remote_ip = Ipv4Addr::random_global();
                let remote_port = rand::random::<u16>() / 2 + 1000;
                let remote_addr = SocketAddrV4::new(remote_ip, remote_port);

                let ipv4_range = Ipv4Range::random_local_subnet();
                let (ipv4_addr_tx, ipv4_addr_rx) = std::sync::mpsc::channel();
                let (spawn_complete, ipv4_plug) = spawn::ipv4_tree(
                    &handle,
                    ipv4_range,
                    node::ipv4::machine(move |ipv4_addr| async move {
                        ipv4_addr_tx.send(ipv4_addr).unwrap();
                        let buffer_out = rand::random::<[u8; 8]>();

                        let mut stream = TcpStream::connect(&SocketAddr::V4(remote_addr))
                            .await
                            .unwrap();
                        stream.write_all(&buffer_out).await.unwrap();
                        trace!("waiting to receive reply");

                        let mut buffer_in = [0u8; 8];
                        stream.read_exact(&mut buffer_in).await.unwrap();
                        assert_eq!(buffer_out, buffer_in);
                    }),
                );

                let (mut plug_tx, mut plug_rx) = ipv4_plug.split();
                let iface_ip = ipv4_addr_rx.recv().unwrap();

                let syn_packet = plug_rx.next().await.unwrap();
                assert_eq!(syn_packet.source_ip(), iface_ip);
                assert_eq!(syn_packet.dest_ip(), remote_ip);

                let tcp = match syn_packet.payload() {
                    Ipv4Payload::Tcp(tcp) => tcp,
                    payload => panic!("unexpected packet payload: {:?}", payload),
                };
                assert_eq!(tcp.dest_port(), remote_port);
                let iface_port = tcp.source_port();
                assert!(tcp.is_syn());

                let init_seq_num_0 = tcp.seq_num();
                let init_seq_num_1 = rand::random::<u32>();
                let window_size = tcp.window_size();

                let ack_packet = Ipv4Packet::new_from_fields_recursive(
                    Ipv4Fields {
                        source_ip: remote_ip,
                        dest_ip: iface_ip,
                        ttl: 12,
                    },
                    Ipv4PayloadFields::Tcp {
                        fields: TcpFields {
                            source_port: remote_addr.port(),
                            dest_port: iface_port,
                            seq_num: init_seq_num_1,
                            ack_num: init_seq_num_0.wrapping_add(1),
                            window_size: window_size,
                            syn: true,
                            ack: true,
                            fin: false,
                            rst: false,
                            ns: false,
                            cwr: false,
                            ece: false,
                            psh: false,
                            urgent: None,
                            mss: None,
                            window_scale: None,
                            selective_ack_permitted: false,
                            selective_acks: None,
                            timestamps: None,
                        },
                        payload: Bytes::new(),
                    },
                );

                trace!("sending SYN-ACK packet");
                plug_tx.send(ack_packet).await.unwrap();
                let mut seq_num_0 = init_seq_num_0.wrapping_add(1);
                let mut seq_num_1 = init_seq_num_1.wrapping_add(1);
                loop {
                    let packet = plug_rx.next().await.unwrap();
                    trace!("received ACK packet: {:?}", packet);
                    assert_eq!(packet.source_ip(), iface_ip);
                    assert_eq!(packet.dest_ip(), remote_ip);
                    let tcp = match packet.payload() {
                        Ipv4Payload::Tcp(tcp) => tcp,
                        payload => panic!("unexpected packet payload: {:?}", payload),
                    };
                    assert_eq!(tcp.dest_port(), remote_port);
                    assert_eq!(tcp.source_port(), iface_port);
                    assert_eq!(tcp.seq_num(), seq_num_0);
                    assert_eq!(tcp.ack_num(), seq_num_1);
                    let next_seq_num_0 = seq_num_0.wrapping_add(tcp.payload().len() as u32);
                    let next_seq_num_1 = seq_num_1.wrapping_add(tcp.payload().len() as u32);
                    if tcp.is_fin() {
                        break;
                    }

                    let ack_packet = Ipv4Packet::new_from_fields_recursive(
                        Ipv4Fields {
                            source_ip: remote_ip,
                            dest_ip: iface_ip,
                            ttl: 12,
                        },
                        Ipv4PayloadFields::Tcp {
                            fields: TcpFields {
                                source_port: remote_addr.port(),
                                dest_port: iface_port,
                                seq_num: seq_num_1,
                                ack_num: next_seq_num_0,
                                window_size: window_size,
                                syn: false,
                                ack: true,
                                fin: false,
                                rst: false,
                                ns: false,
                                cwr: false,
                                ece: false,
                                psh: false,
                                urgent: None,
                                mss: None,
                                window_scale: None,
                                selective_ack_permitted: false,
                                selective_acks: None,
                                timestamps: None,
                            },
                            payload: tcp.payload(),
                        },
                    );

                    plug_tx.send(ack_packet).await.unwrap();

                    seq_num_0 = next_seq_num_0;
                    seq_num_1 = next_seq_num_1;
                }
                let fin_ack_packet = Ipv4Packet::new_from_fields_recursive(
                    Ipv4Fields {
                        source_ip: remote_ip,
                        dest_ip: iface_ip,
                        ttl: 12,
                    },
                    Ipv4PayloadFields::Tcp {
                        fields: TcpFields {
                            source_port: remote_addr.port(),
                            dest_port: iface_port,
                            seq_num: seq_num_1,
                            ack_num: seq_num_0,
                            window_size: window_size,
                            syn: false,
                            ack: true,
                            fin: false,
                            rst: false,
                            ns: false,
                            cwr: false,
                            ece: false,
                            psh: false,
                            urgent: None,
                            mss: None,
                            window_scale: None,
                            selective_ack_permitted: false,
                            selective_acks: None,
                            timestamps: None,
                        },
                        payload: tcp.payload(),
                    },
                );

                plug_tx.send(fin_ack_packet).await.unwrap();
                let fin_packet = Ipv4Packet::new_from_fields_recursive(
                    Ipv4Fields {
                        source_ip: remote_ip,
                        dest_ip: iface_ip,
                        ttl: 12,
                    },
                    Ipv4PayloadFields::Tcp {
                        fields: TcpFields {
                            source_port: remote_addr.port(),
                            dest_port: iface_port,
                            seq_num: seq_num_1,
                            ack_num: seq_num_0,
                            window_size: window_size,
                            syn: false,
                            ack: true,
                            fin: true,
                            rst: false,
                            ns: false,
                            cwr: false,
                            ece: false,
                            psh: false,
                            urgent: None,
                            mss: None,
                            window_scale: None,
                            selective_ack_permitted: false,
                            selective_acks: None,
                            timestamps: None,
                        },
                        payload: tcp.payload(),
                    },
                );

                plug_tx.send(fin_packet).await.unwrap();
                let packet = plug_rx.next().await.unwrap();
                assert_eq!(packet.source_ip(), iface_ip);
                assert_eq!(packet.dest_ip(), remote_ip);
                let tcp = match packet.payload() {
                    Ipv4Payload::Tcp(tcp) => tcp,
                    payload => panic!("unexpected packet payload: {:?}", payload),
                };
                assert_eq!(tcp.dest_port(), remote_port);
                assert_eq!(tcp.source_port(), iface_port);
                assert!(tcp.is_ack());

                spawn_complete.await.unwrap();
            })
        })
    }

    #[test]
    fn test_ping_reply() {
        run_test(3, || {
            let network = Network::new();
            let handle = network.handle();

            task::block_on(async move {
                let (done_tx, done_rx) = oneshot::channel();

                let client_ip = Ipv4Addr::random_global();

                let ipv4_range = Ipv4Range::random_local_subnet();
                let (ipv4_addr_tx, ipv4_addr_rx) = std::sync::mpsc::channel();
                let (spawn_complete, ipv4_plug) = spawn::ipv4_tree(
                    &handle,
                    ipv4_range,
                    node::ipv4::machine(move |ipv4_addr| {
                        ipv4_addr_tx.send(ipv4_addr).unwrap();
                        done_rx
                    }),
                );

                let (mut tx, mut rx) = ipv4_plug.split();
                let iface_ip = ipv4_addr_rx.recv().unwrap();

                let id = rand::random();
                let seq_num = rand::random();
                let payload = Bytes::from(rand::random::<[u8; 8]>().to_vec());
                let ping = Ipv4Packet::new_from_fields_recursive(
                    Ipv4Fields {
                        source_ip: client_ip,
                        dest_ip: iface_ip,
                        ttl: 16,
                    },
                    Ipv4PayloadFields::Icmp {
                        kind: Icmpv4PacketKind::EchoRequest {
                            id,
                            seq_num,
                            payload: payload.clone(),
                        },
                    },
                );

                trace!("start sending packet");
                tx.send(ping).await.unwrap();
                trace!("done sending packet");
                trace!("start receiving packet");
                let packet = rx.next().await.unwrap();
                trace!("done receiving packet");
                let icmp = match packet.payload() {
                    Ipv4Payload::Icmp(icmp) => icmp,
                    payload => panic!("unexpected ipv4 payload kind in reply: {:?}", payload),
                };
                match icmp.kind() {
                    Icmpv4PacketKind::EchoReply {
                        id: reply_id,
                        seq_num: reply_seq_num,
                        payload: reply_payload,
                    } => {
                        assert_eq!(id, reply_id);
                        assert_eq!(seq_num, reply_seq_num);
                        assert_eq!(payload, reply_payload);
                    }
                    kind => panic!("unexpected ICMP reply kind: {:?}", kind),
                }
                done_tx.send(()).unwrap();
                spawn_complete.await.unwrap().unwrap();
            })
        })
    }
}
