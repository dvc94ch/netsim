//! This example demonstrates how to simulate packet loss on some parts of the network.

use async_std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use async_std::task;
use async_timer::timer::PosixTimer;
use futures::channel::oneshot;
use netsim::node::Ipv4Node;
use netsim::{node, spawn, Ipv4Range, Network};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let network = Network::new();
    let handle = network.handle();

    let (server_addr_tx, server_addr_rx) = oneshot::channel();
    let exit = Arc::new(AtomicBool::new(false));
    let exit2 = exit.clone();

    let server_node = node::ipv4::machine(move |ip| {
        println!("[server] ip = {}", ip);

        async move {
            let bind_addr = SocketAddr::V4(SocketAddrV4::new(ip, 0));
            let socket = UdpSocket::bind(&bind_addr).await.unwrap();
            server_addr_tx.send(socket.local_addr().unwrap()).unwrap();

            while !exit.load(Ordering::Relaxed) {
                let mut buf = [0u8; 1024];
                let (_len, _addr) = socket.recv_from(&mut buf).await.unwrap();
                println!("[server] received: packet nr. {}", buf[0]);
            }
        }
    });

    let client_node = node::ipv4::machine(|ip| {
        println!("[client] ip = {}", ip);

        async move {
            let server_addr = server_addr_rx.await.unwrap();
            println!("[client] Got server addr: {}", server_addr);

            let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            for i in 0u8..10 {
                socket.send_to(&[i], &server_addr).await.unwrap();
                PosixTimer::new(Duration::from_millis(500)).await;
            }

            exit2.store(true, Ordering::Relaxed);
        }
    })
    .packet_loss(0.5, Duration::from_millis(500));

    let router_node = node::ipv4::router((server_node, client_node));
    let (spawn_complete, _ipv4_plug) = spawn::ipv4_tree(&handle, Ipv4Range::global(), router_node);

    task::block_on(spawn_complete).unwrap();
}
