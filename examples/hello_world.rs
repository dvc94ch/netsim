//! This is "hello world" of netsim.
//! This example demonstrates how to create a virtual network with one node and send some data
//! to it over UDP.

use async_std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use async_std::task;
use bytes::Bytes;
use futures::channel::oneshot;
use netsim::wire::{Ipv4Fields, Ipv4Packet, Ipv4PayloadFields, UdpFields};
use netsim::{node, spawn, Ipv4Range, Network};
use std::str;

fn main() {
    env_logger::init();
    let network = Network::new();
    let handle = network.handle();

    let (server_addr_tx, server_addr_rx) = oneshot::channel();
    let server_node = node::ipv4::machine(|ip| {
        println!("[server] ip = {}", ip);

        async move {
            let bind_addr = SocketAddr::V4(SocketAddrV4::new(ip, 0));
            let socket = UdpSocket::bind(&bind_addr).await.unwrap();
            // Send server's address to client
            server_addr_tx.send(socket.local_addr().unwrap()).unwrap();

            let mut buf = [0u8; 1024];
            let (len, addr) = socket.recv_from(&mut buf).await.unwrap();
            let s = str::from_utf8(&buf[..len]).unwrap();
            println!("[server] received: {}, from: {}", s, addr);
        }
    });

    // Build and run server node on simulated network
    let (spawn_complete, ipv4_plug) = spawn::ipv4_tree(&handle, Ipv4Range::global(), server_node);

    let (packet_tx, _packet_rx) = ipv4_plug.split();

    task::block_on(async move {
        let server_addr = match server_addr_rx.await.unwrap() {
            SocketAddr::V4(addr) => addr,
            addr => panic!("Ipv6 was not expected: {}", addr),
        };

        // Construct UDP datagram to send to our server node
        let datagram = Ipv4Packet::new_from_fields_recursive(
            Ipv4Fields {
                source_ip: Ipv4Addr::new(78, 1, 2, 3),
                dest_ip: *server_addr.ip(),
                ttl: 10,
            },
            Ipv4PayloadFields::Udp {
                fields: UdpFields {
                    source_port: 12345,
                    dest_port: server_addr.port(),
                },
                payload: Bytes::from("hello world!"),
            },
        );

        // Send datagram to our server via IPv4 plug
        packet_tx.unbounded_send(datagram);

        // Wait till server node is finished
        spawn_complete.await.unwrap();
    });
}
