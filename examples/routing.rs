//! This example demonstrates how to create a virtual network with two virtual devices, connect
//! them together with virtual router and exchange data using `std::net::UdpSocket`.

use async_std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use async_std::task;
use futures::channel::oneshot;
use netsim::{node, Ipv4Range, Network};
use std::str;

fn main() {
    let network = Network::new();
    let network_handle = network.handle();

    let (server_addr_tx, server_addr_rx) = oneshot::channel();
    let server_node = node::ipv4::machine(|ip| {
        println!("[server] ip = {}", ip);

        async move {
            let bind_addr = SocketAddr::V4(SocketAddrV4::new(ip, 0));
            let socket = UdpSocket::bind(&bind_addr).await.unwrap();
            server_addr_tx.send(socket.local_addr().unwrap()).unwrap();

            let mut buf = [0u8; 1024];
            let (len, addr) = socket.recv_from(&mut buf).await.unwrap();
            let s = str::from_utf8(&buf[..len]).unwrap();
            println!("[server] received: {}, from: {}", s, addr);
        }
    });

    let client_node = node::ipv4::machine(|ip| {
        println!("[client] ip = {}", ip);

        async move {
            let server_addr = server_addr_rx.await.unwrap();
            println!("[client] Got server addr: {}", server_addr);

            let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            socket.send_to(b"hello world!", &server_addr).await.unwrap();
        }
    });

    let router_node = node::ipv4::router((server_node, client_node));
    let (spawn_complete, _ipv4_plug) =
        network_handle.spawn_ipv4_tree(Ipv4Range::global(), router_node);

    task::block_on(spawn_complete).unwrap();
}
