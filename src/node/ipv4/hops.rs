use super::Ipv4Node;
use crate::network::NetworkHandle;
use crate::range::Ipv4Range;
use crate::spawn_complete::SpawnComplete;
use crate::wire::Ipv4Plug;

/// A node representing hops between Ipv4 nodes.
pub struct HopsNode<N> {
    node: N,
    num_hops: u32,
}

/// Add hops between nodes. The will cause the TTL of packets travelling on this connection to
/// decrease by the given amount.
pub fn hops<N>(num_hops: u32, node: N) -> HopsNode<N>
where
    N: Ipv4Node,
{
    HopsNode { node, num_hops }
}

impl<N> Ipv4Node for HopsNode<N>
where
    N: Ipv4Node,
{
    type Output = N::Output;

    fn build(
        self,
        handle: &NetworkHandle,
        ipv4_range: Ipv4Range,
    ) -> (SpawnComplete<N::Output>, Ipv4Plug) {
        let (spawn_complete, plug) = self.node.build(handle, ipv4_range);
        let plug = plug.with_hops(handle, self.num_hops);
        (spawn_complete, plug)
    }
}
