use super::Ipv6Node;
use crate::device::MachineBuilder;
use crate::iface::IpIfaceBuilder;
use crate::network::NetworkHandle;
use crate::range::Ipv6Range;
use crate::route::Ipv6Route;
use crate::spawn_complete::SpawnComplete;
use crate::wire::{IpPlug, Ipv6Plug};
use async_std::net::Ipv6Addr;
use futures::future::Future;

/// A node representing an Ipv6 machine.
pub struct MachineNode<F> {
    func: F,
}

/// Create a node for an Ipv6 machine. This node will run the given function in a network
/// namespace with a single interface.
pub fn machine<T, F>(func: F) -> MachineNode<F>
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
    F: FnOnce(Ipv6Addr) -> T + Send + 'static,
{
    MachineNode { func }
}

impl<T, F> Ipv6Node for MachineNode<F>
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
    F: FnOnce(Ipv6Addr) -> T + Send + 'static,
{
    type Output = T::Output;

    fn build(
        self,
        handle: &NetworkHandle,
        ipv6_range: Ipv6Range,
    ) -> (SpawnComplete<T::Output>, Ipv6Plug) {
        let address = ipv6_range.random_client_addr();
        let iface = {
            IpIfaceBuilder::new()
                .ipv6_addr(address, ipv6_range.netmask_prefix_length())
                .ipv6_route(Ipv6Route::new(Ipv6Range::global(), ipv6!("::")))
        };
        let (plug_a, plug_b) = IpPlug::new_pair();

        let spawn_complete = {
            MachineBuilder::new()
                .add_ip_iface(iface, plug_b)
                .spawn(handle, move || (self.func)(address))
        };

        let plug_a = plug_a.into_ipv6_plug(handle);

        (spawn_complete, plug_a)
    }
}
