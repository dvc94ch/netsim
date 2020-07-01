use crate::network::NetworkHandle;
use crate::plug::Plug;
use crate::util;
use futures::future::Future;
use futures::stream::Stream;
use rand::distributions::{Distribution, Uniform};
use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

/// Simulate packet loss on a link
pub struct PacketLoss<T: Unpin + fmt::Debug + 'static> {
    plug_a: Plug<T>,
    plug_b: Plug<T>,
    mean_loss_duration: Duration,
    mean_keep_duration: Duration,
    currently_losing: bool,
    state_toggle_time: Instant,
}

impl<T: Unpin + fmt::Debug + Send + 'static> PacketLoss<T> {
    /// Spawn a `PacketLoss` directly onto the event loop
    pub fn spawn(
        handle: &NetworkHandle,
        loss_rate: f64,
        mean_loss_duration: Duration,
        plug_a: Plug<T>,
        plug_b: Plug<T>,
    ) {
        let mean_keep_duration = mean_loss_duration.mul_f64(1.0 / loss_rate - 1.0);
        let currently_losing = Uniform::new(0.0, 1.0).sample(&mut rand::thread_rng()) < loss_rate;
        let state_toggle_time = Instant::now()
            + if currently_losing {
                mean_loss_duration.mul_f64(util::expovariate_rand())
            } else {
                mean_keep_duration.mul_f64(util::expovariate_rand())
            };
        let packet_loss = Self {
            plug_a,
            plug_b,
            mean_loss_duration,
            mean_keep_duration,
            currently_losing,
            state_toggle_time,
        };
        handle.spawn(packet_loss);
    }
}

impl<T: Unpin + fmt::Debug + 'static> Future for PacketLoss<T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let now = Instant::now();
        while self.state_toggle_time < now {
            self.currently_losing = !self.currently_losing;
            let state_toggle_time_increase = if self.currently_losing {
                self.mean_loss_duration.mul_f64(util::expovariate_rand())
            } else {
                self.mean_keep_duration.mul_f64(util::expovariate_rand())
            };
            self.state_toggle_time += state_toggle_time_increase;
        }

        let a_unplugged = loop {
            match Pin::new(&mut self.plug_a.rx).poll_next(cx) {
                Poll::Pending => break false,
                Poll::Ready(None) => break true,
                Poll::Ready(Some(packet)) => {
                    if self.currently_losing {
                        trace!("packet loss randomly dropping packet: {:?}", packet);
                    } else {
                        let _ = self.plug_b.tx.unbounded_send(packet);
                    }
                }
            }
        };

        let b_unplugged = loop {
            match Pin::new(&mut self.plug_b.rx).poll_next(cx) {
                Poll::Pending => break false,
                Poll::Ready(None) => break true,
                Poll::Ready(Some(packet)) => {
                    if self.currently_losing {
                        trace!("packet loss randomly dropping packet: {:?}", packet);
                    } else {
                        let _ = self.plug_a.tx.unbounded_send(packet);
                    }
                }
            }
        };

        if a_unplugged && b_unplugged {
            return Poll::Ready(());
        }

        Poll::Pending
    }
}
