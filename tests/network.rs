#![cfg(feature = "linux_host")]

#[macro_use]
extern crate net_literals;

use async_std::task;
use futures::channel::oneshot;
use netsim::{node, Ipv4Range, Network};

#[test]
fn spawn_ipv4_tree() {
    let network = Network::new();
    let network_handle = network.handle();

    let (addr_tx, addr_rx) = oneshot::channel();

    let node = node::ipv4::machine(|ip| async move {
        addr_tx.send(ip).unwrap();
    });
    let addr = task::block_on(async move {
        let (spawn_complete, _ipv4_plug) =
            network_handle.spawn_ipv4_tree(Ipv4Range::new(ipv4!("78.100.10.1"), 30), node);
        spawn_complete.await.unwrap();
        addr_rx.await.unwrap()
    });

    assert_eq!(addr.octets()[0..3], [78, 100, 10]);
}
