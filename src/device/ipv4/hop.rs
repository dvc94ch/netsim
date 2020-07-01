use crate::network::NetworkHandle;
use crate::wire::{Ipv4Fields, Ipv4Plug};
use futures::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Connects two `Ipv4Plug`s and adds a hop between them. This causes the TTL value of all packets
/// to be decremented while travelling along the connection (and dropped if the TLL reaches zero).
pub struct Ipv4Hop {
    plug_a: Ipv4Plug,
    plug_b: Ipv4Plug,
}

impl Ipv4Hop {
    /// Create a new hop by connecting the two given plugs.
    pub fn new(plug_a: Ipv4Plug, plug_b: Ipv4Plug) -> Ipv4Hop {
        Ipv4Hop { plug_a, plug_b }
    }

    /// Create a new hop by connecting the two given plugs. Spawn the `Hopv4` directly onto the
    /// tokio event loop.
    pub fn spawn(handle: &NetworkHandle, plug_a: Ipv4Plug, plug_b: Ipv4Plug) {
        let hop_v4 = Ipv4Hop::new(plug_a, plug_b);
        handle.spawn(hop_v4);
    }
}

impl Future for Ipv4Hop {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let a_unplugged = loop {
            match self.plug_a.poll_incoming(cx) {
                Poll::Pending => break false,
                Poll::Ready(None) => break true,
                Poll::Ready(Some(mut packet)) => {
                    let next_ttl = match packet.ttl().checked_sub(1) {
                        Some(ttl) => ttl,
                        None => {
                            info!("hop dropping packet due to expired ttl: {:?}", packet);
                            continue;
                        }
                    };
                    let fields = packet.fields();
                    packet.set_fields(Ipv4Fields {
                        ttl: next_ttl,
                        ..fields
                    });
                    let _ = self.plug_b.unbounded_send(packet);
                }
            }
        };

        let b_unplugged = loop {
            match self.plug_b.poll_incoming(cx) {
                Poll::Pending => break false,
                Poll::Ready(None) => break true,
                Poll::Ready(Some(mut packet)) => {
                    let next_ttl = match packet.ttl().checked_sub(1) {
                        Some(ttl) => ttl,
                        None => {
                            info!("hop dropping packet due to expired ttl: {:?}", packet);
                            continue;
                        }
                    };
                    let fields = packet.fields();
                    packet.set_fields(Ipv4Fields {
                        ttl: next_ttl,
                        ..fields
                    });
                    let _ = self.plug_a.unbounded_send(packet);
                }
            }
        };

        if a_unplugged && b_unplugged {
            return Poll::Ready(());
        }

        Poll::Pending
    }
}
