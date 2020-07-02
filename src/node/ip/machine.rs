use super::IpNode;
use crate::device::MachineBuilder;
use crate::iface::IpIfaceBuilder;
use crate::network::NetworkHandle;
use crate::range::{Ipv4Range, Ipv6Range};
use crate::route::{Ipv4Route, Ipv6Route};
use crate::spawn_complete::SpawnComplete;
use crate::wire::IpPlug;
use async_std::net::{Ipv4Addr, Ipv6Addr};
use futures::future::Future;

/// A node representing a machine with an IP interface.
pub struct MachineNode<F> {
    func: F,
}

/// Create a node for a machine with an IP interface. This node will run the given function in a
/// network namespace with a single interface.
pub fn machine<T, F>(func: F) -> MachineNode<F>
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
    F: FnOnce(Option<Ipv4Addr>, Option<Ipv6Addr>) -> T + Send + 'static,
{
    MachineNode { func }
}

impl<T, F> IpNode for MachineNode<F>
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
    F: FnOnce(Option<Ipv4Addr>, Option<Ipv6Addr>) -> T + Send + 'static,
{
    type Output = T::Output;

    fn build(
        self,
        handle: &NetworkHandle,
        ipv4_range: Option<Ipv4Range>,
        ipv6_range: Option<Ipv6Range>,
    ) -> (SpawnComplete<T::Output>, IpPlug) {
        let mut iface = IpIfaceBuilder::new();
        let ipv4_addr = if let Some(range) = ipv4_range {
            let ipv4_addr = range.random_client_addr();
            iface = {
                iface
                    .ipv4_addr(ipv4_addr, range.netmask_prefix_length())
                    .ipv4_route(Ipv4Route::new(Ipv4Range::global(), None))
            };
            Some(ipv4_addr)
        } else {
            None
        };
        let ipv6_addr = if let Some(range) = ipv6_range {
            let ipv6_addr = range.random_client_addr();
            iface = {
                iface
                    .ipv6_addr(ipv6_addr, range.netmask_prefix_length())
                    .ipv6_route(Ipv6Route::new(Ipv6Range::global(), ipv6!("::")))
            };
            Some(ipv6_addr)
        } else {
            None
        };
        let (plug_a, plug_b) = IpPlug::new_pair();

        let spawn_complete = {
            MachineBuilder::new()
                .add_ip_iface(iface, plug_b)
                .spawn(handle, move || (self.func)(ipv4_addr, ipv6_addr))
        };

        (spawn_complete, plug_a)
    }
}
