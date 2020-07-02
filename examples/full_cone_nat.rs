use async_std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use async_std::task;
use futures::channel::oneshot;
use netsim::device::ipv4::Ipv4NatBuilder;
use netsim::{node, spawn, Ipv4Range, Network};
use std::str;

fn main() {
    let network = Network::new();
    let handle = network.handle();

    let (server_addr_tx, server_addr_rx) = oneshot::channel();
    let (client0_addr_tx, client0_addr_rx) = oneshot::channel();
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
            client0_addr_tx.send(addr).unwrap();
        }
    });

    let client0_node = node::ipv4::machine(|ip| {
        println!("[client0] ip = {}", ip);

        async move {
            let server_addr = server_addr_rx.await.unwrap();
            println!("[client0] Got server addr: {}", server_addr);

            let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            socket.send_to(b"hello world", &server_addr).await.unwrap();
            let mut buf = [0u8; 1024];
            let (len, addr) = socket.recv_from(&mut buf).await.unwrap();
            let s = str::from_utf8(&buf[..len]).unwrap();
            println!("[client0] received: {}, from: {}", s, addr);
        }
    });

    let nat_builder = Ipv4NatBuilder::new().subnet(Ipv4Range::local_subnet_192(0));
    let client0_behind_nat_node = node::ipv4::nat(nat_builder, client0_node);

    let client1_node = node::ipv4::machine(|ip| {
        println!("[client1] ip = {}", ip);

        async move {
            let client0_addr = client0_addr_rx.await.unwrap();
            println!("[client1] Got client0 addr: {}", client0_addr);

            let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            socket
                .send_to(b"this is client1!", &client0_addr)
                .await
                .unwrap();
        }
    });

    let router_node = node::ipv4::router((server_node, client0_behind_nat_node, client1_node));
    let (spawn_complete, _ipv4_plug) = spawn::ipv4_tree(&handle, Ipv4Range::global(), router_node);

    task::block_on(spawn_complete).unwrap();
}
