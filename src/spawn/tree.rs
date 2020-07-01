use crate::network::NetworkHandle;
use crate::node::{EtherNode, IpNode, Ipv4Node, Ipv6Node};
use crate::range::{Ipv4Range, Ipv6Range};
use crate::spawn_complete::SpawnComplete;
use crate::wire::{EtherPlug, IpPlug, Ipv4Plug, Ipv6Plug};

/// Spawn a hierarchical network of nodes. The returned plug can be used to write frames to the
/// network and read frames that try to leave the network.
pub fn ether_tree<N: EtherNode>(
    handle: &NetworkHandle,
    ipv4_range: Option<Ipv4Range>,
    ipv6_range: Option<Ipv6Range>,
    node: N,
) -> (SpawnComplete<N::Output>, EtherPlug) {
    node.build(handle, ipv4_range, ipv6_range)
}

/// Spawn a hierarchical network of nodes. The returned plug can be used to write packets to the
/// network and read packets that try to leave the network.
pub fn ip_tree<N: IpNode>(
    handle: &NetworkHandle,
    ipv4_range: Option<Ipv4Range>,
    ipv6_range: Option<Ipv6Range>,
    node: N,
) -> (SpawnComplete<N::Output>, IpPlug) {
    node.build(handle, ipv4_range, ipv6_range)
}

/// Spawn a hierarchical network of nodes. The returned plug can be used to write packets to the
/// network and read packets that try to leave the network.
pub fn ipv4_tree<N: Ipv4Node>(
    handle: &NetworkHandle,
    ipv4_range: Ipv4Range,
    node: N,
) -> (SpawnComplete<N::Output>, Ipv4Plug) {
    node.build(handle, ipv4_range)
}

/// Spawn a hierarchical network of nodes. The returned plug can be used to write packets to the
/// network and read packets that try to leave the network.
pub fn ipv6_tree<N: Ipv6Node>(
    handle: &NetworkHandle,
    ipv6_range: Ipv6Range,
    node: N,
) -> (SpawnComplete<N::Output>, Ipv6Plug) {
    node.build(handle, ipv6_range)
}
