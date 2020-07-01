use crate::network::NetworkHandle;
use crate::wire::EtherPlug;
use futures::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Used to build a `Hub`
#[derive(Default)]
pub struct HubBuilder {
    connections: Vec<EtherPlug>,
}

impl HubBuilder {
    /// Create a new `HubBuilder`
    pub fn new() -> HubBuilder {
        HubBuilder {
            connections: Vec::new(),
        }
    }

    /// Add a peer to the hub.
    pub fn connect(mut self, plug: EtherPlug) -> HubBuilder {
        self.connections.push(plug);
        self
    }

    /// Build the `Hub`.
    pub fn build(self) -> Hub {
        Hub::new(self.connections)
    }

    /// Build the `Hub` on the event loop.
    pub fn spawn(self, handle: &NetworkHandle) {
        Hub::spawn(handle, self.connections)
    }
}

/// An ethernet hub. This is a stupid hub which just blindly forwards all incoming frames to all
/// connected peers.
pub struct Hub {
    connections: Vec<EtherPlug>,
}

impl Hub {
    /// Create a new ethernet hub with the given clients connected to it.
    pub fn new(connections: Vec<EtherPlug>) -> Hub {
        Hub { connections }
    }

    /// Spawn a new ethernet hub on the event loop.
    pub fn spawn(handle: &NetworkHandle, connections: Vec<EtherPlug>) {
        let hub = Hub::new(connections);
        handle.spawn(hub);
    }
}

impl Future for Hub {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut all_disconnected = true;
        for i in 0..self.connections.len() {
            all_disconnected &= loop {
                match self.connections[i].poll_incoming(cx) {
                    Poll::Pending => break false,
                    Poll::Ready(None) => break true,
                    Poll::Ready(Some(packet)) => {
                        for connection in &mut self.connections {
                            let _ = connection.unbounded_send(packet.clone());
                        }
                    }
                }
            };
        }

        if all_disconnected {
            return Poll::Ready(());
        }

        Poll::Pending
    }
}
