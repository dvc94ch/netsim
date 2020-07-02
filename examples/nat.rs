//! This a basic example how to setup NAT (Network Address Translation).
//!
//! 1. it creates publicly accessible server node.
//! 2. client node is created and put under NAT
//! 3. client connects to the server
//!
//! When you run the example, you can see that client node sees its LAN IP address and when it
//! connects to the server, server sees its public IP - one that NAT device owns.

use async_std::net::{TcpListener, TcpStream};
use async_std::task;
use futures::channel::oneshot;
use futures::StreamExt;
use netsim::device::ipv4::Ipv4NatBuilder;
use netsim::{node, spawn, Ipv4Range, Network};
use std::net::{SocketAddr, SocketAddrV4};
use std::thread;

fn main() {
    let network = Network::new();
    let handle = network.handle();

    let (server_addr_tx, server_addr_rx) = oneshot::channel();
    let server = node::ipv4::machine(move |ip| {
        // This code is run on a separate thread.
        println!(
            "[server] ip = {}, thread = {:?}",
            ip,
            thread::current().id()
        );

        async move {
            let bind_addr = SocketAddr::V4(SocketAddrV4::new(ip, 0));
            let listener = TcpListener::bind(&bind_addr).await.unwrap();
            server_addr_tx.send(listener.local_addr().unwrap()).unwrap();
            let mut incoming = listener.incoming();

            while let Some(incoming) = incoming.next().await {
                let incoming = incoming.unwrap();
                println!(
                    "[server] Client connected: {}",
                    incoming.peer_addr().unwrap()
                );
            }
        }
    });

    let client = node::ipv4::machine(move |ip| {
        // This code is run on a separate thread.
        println!(
            "[client] ip = {}, thread = {:?}",
            ip,
            thread::current().id()
        );

        async move {
            let server_addr = server_addr_rx.await.unwrap();
            println!("[client] Got server addr: {}", server_addr);

            TcpStream::connect(&server_addr).await.unwrap();
            println!("[client] Connected to server");
        }
    });

    let client = node::ipv4::nat(Ipv4NatBuilder::default(), client);

    let router = node::ipv4::router((server, client));
    let (spawn_complete, _ipv4_plug) = spawn::ipv4_tree(&handle, Ipv4Range::global(), router);

    task::block_on(spawn_complete).unwrap();
}
