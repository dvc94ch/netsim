//! This example demonstrates how to artificially introduce latency on virtual network devices.

use async_std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use async_std::task;
use futures::channel::oneshot;
use netsim::node::Ipv4Node;
use netsim::{node, spawn, Ipv4Range, Network};
use std::str;
use std::time::{Duration, Instant};

fn main() {
    let network = Network::new();
    let handle = network.handle();

    let (server_addr_tx, server_addr_rx) = oneshot::channel();
    let server_node = node::ipv4::machine(move |ip| {
        println!("[server] ip = {}", ip);

        async move {
            let start_time = Instant::now();
            let bind_addr = SocketAddr::V4(SocketAddrV4::new(ip, 0));
            let socket = UdpSocket::bind(&bind_addr).await.unwrap();
            server_addr_tx.send(socket.local_addr().unwrap()).unwrap();

            let mut buf = [0u8; 1024];
            let (len, addr) = socket.recv_from(&mut buf).await.unwrap();
            let s = str::from_utf8(&buf[..len]).unwrap();
            let latency = start_time.elapsed();
            println!(
                "[server] received: {}, from: {}, latency: {:?}",
                s, addr, latency
            );
        }
    });

    let client_node = node::ipv4::machine(|ip| {
        println!("[client] ip = {}", ip);

        async move {
            let server_addr = server_addr_rx.await.unwrap();
            let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            socket.send_to(b"hello world!", &server_addr).await.unwrap();
        }
    })
    .latency(Duration::from_secs(2), Duration::from_millis(100));

    let router_node = node::ipv4::router((server_node, client_node));
    let (spawn_complete, _ipv4_plug) = spawn::ipv4_tree(&handle, Ipv4Range::global(), router_node);

    task::block_on(spawn_complete).unwrap();
}
