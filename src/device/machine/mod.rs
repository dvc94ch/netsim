use self::tap::TapTask;
use self::tun::TunTask;
use crate::iface::{EtherIfaceBuilder, IpIfaceBuilder};
use crate::network::NetworkHandle;
use crate::spawn;
use crate::spawn_complete::SpawnComplete;
use crate::wire::{EtherPlug, IpPlug};
use futures::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

mod tap;
mod tun;

#[derive(Default)]
/// A builder for creating a virtual network machine.
/// Machines are simulated using Linux TUN/TAP devices and network namespaces.
pub struct MachineBuilder {
    ether_ifaces: Vec<(EtherIfaceBuilder, EtherPlug)>,
    ip_ifaces: Vec<(IpIfaceBuilder, IpPlug)>,
}

impl MachineBuilder {
    /// Create a new `MachineBuilder` with no interfaces.
    pub fn new() -> MachineBuilder {
        MachineBuilder::default()
    }

    /// Add an ethernet (TAP) interface to the machine
    pub fn add_ether_iface(mut self, iface: EtherIfaceBuilder, plug: EtherPlug) -> MachineBuilder {
        self.ether_ifaces.push((iface, plug));
        self
    }

    /// Add an IP (TUN) interface to the machine
    pub fn add_ip_iface(mut self, iface: IpIfaceBuilder, plug: IpPlug) -> MachineBuilder {
        self.ip_ifaces.push((iface, plug));
        self
    }

    /// Spawn the machine onto the event loop. The returned `SpawnComplete` will resolve with the
    /// value returned by the given function.
    pub fn spawn<F, T>(self, handle: &NetworkHandle, func: F) -> SpawnComplete<T::Output>
    where
        F: FnOnce() -> T,
        F: Send + 'static,
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        let exit = Arc::new(AtomicBool::new(false));
        let (ether_tx, ether_rx) = std::sync::mpsc::channel();
        let (ip_tx, ip_rx) = std::sync::mpsc::channel();

        let exit2 = exit.clone();
        let spawn_complete = spawn::new_namespace(move || {
            for (iface, plug) in self.ether_ifaces {
                let tap_unbound = unwrap!(iface.build_unbound());
                unwrap!(ether_tx.send((tap_unbound, plug)));
            }
            drop(ether_tx);

            for (iface, plug) in self.ip_ifaces {
                let tun_unbound = unwrap!(iface.build_unbound());
                unwrap!(ip_tx.send((tun_unbound, plug)));
            }
            drop(ip_tx);

            async_std::task::block_on(async move {
                let ret = func().await;
                exit2.store(true, Ordering::Relaxed);
                ret
            })
        });

        for (tap_unbound, plug) in ether_rx {
            let tap = tap_unbound.bind();
            let task = TapTask::new(tap, plug, exit.clone());
            handle.spawn(task);
        }

        for (tun_unbound, plug) in ip_rx {
            let tun = tun_unbound.bind();
            let task = TunTask::new(tun, plug, exit.clone());
            handle.spawn(task);
        }

        spawn_complete
    }
}
