use crate::iface::IpIface;
use crate::wire::{IpPacket, IpPlug, IpReceiver, IpSender};
use async_timer::timer::{PosixTimer as Timer, Timer as _};
use futures::future::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use std::mem;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

pub struct TunTask {
    tun: IpIface,
    packet_tx: IpSender,
    packet_rx: IpReceiver,
    sending_packet: Option<IpPacket>,
    state: TunTaskState,
}

impl TunTask {
    pub fn new(tun: IpIface, plug: IpPlug, exit: Arc<AtomicBool>) -> TunTask {
        trace!("TunTask: creating");
        let (tx, rx) = plug.split();
        TunTask {
            tun,
            packet_tx: tx,
            packet_rx: rx,
            sending_packet: None,
            state: TunTaskState::Receiving { exit },
        }
    }
}

enum TunTaskState {
    Receiving { exit: Arc<AtomicBool> },
    Dying(Timer),
    Invalid,
}

impl Future for TunTask {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        trace!("TunTask: polling");
        let grace_period = Duration::from_millis(100);

        let mut received_frames = false;
        loop {
            trace!("TunTask: looping receiver");

            match Pin::new(&mut self.tun).poll_next(cx) {
                Poll::Ready(Some(Ok(frame))) => {
                    trace!("TunTask: received packet");
                    self.packet_tx.unbounded_send(frame);
                    received_frames = true;
                }
                Poll::Ready(None) => {
                    panic!("TAP stream ended somehow");
                }
                Poll::Pending => break,
                Poll::Ready(Some(Err(e))) => {
                    panic!("reading TAP device yielded an error: {}", e);
                }
            }
        }
        trace!("TunTask: done looping receiver");

        loop {
            trace!("TunTask: looping sender ...");

            loop {
                match Pin::new(&mut self.tun).poll_ready(cx) {
                    Poll::Ready(Ok(())) => {}
                    Poll::Pending => {
                        trace!("TunTask: TUN sink not ready");
                        break;
                    }
                    Poll::Ready(Err(e)) => {
                        panic!("writing TAP device yielded an error: {}", e);
                    }
                };

                if let Some(frame) = self.sending_packet.take() {
                    trace!("TunTask: sending packet");
                    if let Err(e) = Pin::new(&mut self.tun).start_send(frame) {
                        panic!("completing TAP device write yielded an error: {}", e);
                    }
                } else {
                    break;
                }
            }

            if self.sending_packet.is_none() {
                match Pin::new(&mut self.packet_rx).poll_next(cx) {
                    Poll::Ready(Some(packet)) => {
                        trace!("TunTask: added packet to queue");
                        self.sending_packet = Some(packet);
                    }
                    Poll::Pending => {
                        trace!("TunTask: no frames");
                        break;
                    }
                    Poll::Ready(None) => unreachable!(),
                }
            }
        }
        trace!("TunTask: done looping sender");

        let mut state = mem::replace(&mut self.state, TunTaskState::Invalid);
        loop {
            match state {
                TunTaskState::Receiving { ref exit } => {
                    trace!("TunTask: state == receiving");
                    if exit.load(Ordering::Relaxed) {
                        state = TunTaskState::Dying(Timer::new(grace_period));
                        continue;
                    } else {
                        break;
                    }
                }
                TunTaskState::Dying(mut timeout) => {
                    trace!("TunTask: state == dying");
                    if received_frames {
                        timeout.restart(grace_period);
                    }
                    match Pin::new(&mut timeout).poll(cx) {
                        Poll::Ready(()) => {
                            return Poll::Ready(());
                        }
                        Poll::Pending => {
                            state = TunTaskState::Dying(timeout);
                            break;
                        }
                    }
                }
                TunTaskState::Invalid => {
                    panic!("TunTask in invalid state!");
                }
            }
        }
        self.state = state;

        trace!("TunTask: exiting");
        Poll::Pending
    }
}
