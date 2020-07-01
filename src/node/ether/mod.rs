use crate::network::NetworkHandle;
use crate::range::{Ipv4Range, Ipv6Range};
use crate::spawn_complete::SpawnComplete;
use crate::wire::EtherPlug;

mod hub;
mod machine;

pub use self::hub::{hub, HubNode};
pub use self::machine::{machine, MachineNode};

/// An `EtherNode` describes a recipe for constructing a network when given the IP ranges that the network
/// should operate on.
pub trait EtherNode: Sized {
    /// The type returned by the thread spawned by this node.
    type Output: Unpin + Send + 'static;

    /// Consume the `EtherNode` and build the network it describes. Returns a `SpawnComplete` that can
    /// be used to join the spawned thread and an `EtherPlug` that can be used to read-write frames to
    /// the head node of the network.
    fn build(
        self,
        handle: &NetworkHandle,
        ipv4_range: Option<Ipv4Range>,
        ipv6_range: Option<Ipv6Range>,
    ) -> (SpawnComplete<Self::Output>, EtherPlug);
}
