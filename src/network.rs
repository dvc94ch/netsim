use crate::node::{Ipv4Node, Ipv6Node};
use crate::range::{Ipv4Range, Ipv6Range};
use crate::spawn_complete::SpawnComplete;
use crate::wire::{Ipv4Plug, Ipv6Plug};
use futures::future::{poll_fn, Future};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::Poll;

/// A `Network` manages a set of tasks/devices. Dropping the `Network` will destroy all associated
/// tasks/devices.
pub struct Network {
    exit: Arc<AtomicBool>,
}

impl Network {
    /// Create a new `Network` running on the given event loop.
    pub fn new() -> Network {
        Network {
            exit: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a handle to the network. Can be used to spawn tasks to the network.
    pub fn handle(&self) -> NetworkHandle {
        NetworkHandle {
            exit: self.exit.clone(),
        }
    }

    /// Spawn a hierarchical network of nodes. The returned plug can be used to write packets to the
    /// network and read packets that try to leave the network.
    pub fn spawn_ipv4_tree<N: Ipv4Node>(
        &self,
        ipv4_range: Ipv4Range,
        node: N,
    ) -> (SpawnComplete<N::Output>, Ipv4Plug) {
        node.build(&self.handle(), ipv4_range)
    }

    /// Spawn a hierarchical network of nodes. The returned plug can be used to write packets to the
    /// network and read packets that try to leave the network.
    pub fn spawn_ipv6_tree<N: Ipv6Node>(
        &self,
        ipv6_range: Ipv6Range,
        node: N,
    ) -> (SpawnComplete<N::Output>, Ipv6Plug) {
        node.build(&self.handle(), ipv6_range)
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        self.exit.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone)]
/// A handle to a `Network`
pub struct NetworkHandle {
    exit: Arc<AtomicBool>,
}

impl NetworkHandle {
    /// Spawn a future to the event loop. The future will be cancelled when the `Network` is
    /// destroyed,
    pub fn spawn<F>(&self, mut f: F)
    where
        F: Future<Output = ()> + Unpin + Send + 'static,
    {
        let exit = self.exit.clone();

        async_std::task::spawn(poll_fn(move |cx| {
            while !exit.load(Ordering::Relaxed) {
                if Pin::new(&mut f).poll(cx) == Poll::Pending {
                    return Poll::Pending;
                }
            }
            Poll::Ready(())
        }));
    }

    /// Spawn a hierarchical network of nodes. The returned plug can be used to write packets to the
    /// network and read packets that try to leave the network.
    pub fn spawn_ipv4_tree<N: Ipv4Node>(
        &self,
        ipv4_range: Ipv4Range,
        node: N,
    ) -> (SpawnComplete<N::Output>, Ipv4Plug) {
        node.build(&self, ipv4_range)
    }
}
