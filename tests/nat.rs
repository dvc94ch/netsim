#![cfg(feature = "linux_host")]

use async_std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use async_std::task;
use futures::channel::mpsc;
use futures::future::FutureExt;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use netsim::device::ipv4::Ipv4NatBuilder;
use netsim::{node, Ipv4Range, Network};

/// Makes 3 UDP queries from the same client to different servers and returns the ports the server
/// saw the client.
fn query_under_nat(nat_builder: Ipv4NatBuilder) -> Vec<u16> {
    let network = Network::new();
    let network_handle = network.handle();

    let (stun_addrs_tx, mut stun_addrs_rx) = mpsc::channel(1);
    let (client_ports_tx, mut client_ports_rx) = mpsc::channel(1);

    let mut stun_servers = vec![];
    for _ in 0..3 {
        let mut stun_addrs_tx = stun_addrs_tx.clone();
        let mut client_ports_tx = client_ports_tx.clone();

        let server = node::ipv4::machine(move |ip| async move {
            let bind_addr = SocketAddr::V4(SocketAddrV4::new(ip, 0));
            let sock = UdpSocket::bind(bind_addr).await.unwrap();
            let pub_addr = sock.local_addr().unwrap();
            stun_addrs_tx.send(pub_addr).await.unwrap();

            let mut buf = [0; 4096];
            let (_, client_addr) = sock.recv_from(&mut buf).await.unwrap();
            client_ports_tx.send(client_addr.port()).await.unwrap();
        });
        stun_servers.push(server);
    }

    let client = node::ipv4::machine(move |_ip| async move {
        let sock = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        while let Some(Some(addr)) = stun_addrs_rx.next().now_or_never() {
            sock.send_to(&[1, 2, 3], addr).await.unwrap();
        }
    });
    let client = node::ipv4::nat(nat_builder, client);

    let router = node::ipv4::router(stun_servers);
    let router = node::ipv4::router((router, client));
    let spawn_complete = async move {
        let (spawn_complete, _ip_plug) =
            network_handle.spawn_ipv4_tree(Ipv4Range::global(), router);
        spawn_complete.await
    };
    task::block_on(spawn_complete).unwrap();

    let mut ports = vec![];
    while let Some(Some(port)) = client_ports_rx.next().now_or_never() {
        ports.push(port);
    }
    ports
}

#[test]
fn default_nat_is_full_cone() {
    let ports = query_under_nat(Ipv4NatBuilder::default());

    assert_eq!(ports[0], ports[1]);
    assert_eq!(ports[1], ports[2]);
}

#[test]
fn symmetric_nat_assigns_different_port_for_different_endpoint() {
    let ports = query_under_nat(Ipv4NatBuilder::default().symmetric());

    assert_ne!(ports[0], ports[1]);
    assert_ne!(ports[0], ports[2]);
    assert_ne!(ports[1], ports[2]);
}
