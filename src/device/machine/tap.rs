use crate::iface::EtherIface;
use crate::wire::{EtherFrame, EtherPlug};
use async_timer::timer::{PosixTimer as Timer, Timer as _};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::future::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use std::mem;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

pub struct TapTask {
    tap: EtherIface,
    frame_tx: UnboundedSender<EtherFrame>,
    frame_rx: UnboundedReceiver<EtherFrame>,
    sending_frame: Option<EtherFrame>,
    state: TapTaskState,
}

impl TapTask {
    pub fn new(tap: EtherIface, plug: EtherPlug, exit: Arc<AtomicBool>) -> TapTask {
        let (tx, rx) = plug.split();
        TapTask {
            tap,
            frame_tx: tx,
            frame_rx: rx,
            sending_frame: None,
            state: TapTaskState::Receiving { exit },
        }
    }
}

enum TapTaskState {
    Receiving { exit: Arc<AtomicBool> },
    Dying(Timer),
    Invalid,
}

impl Future for TapTask {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let grace_period: Duration = Duration::from_millis(100);

        let mut received_frames = false;
        loop {
            match Pin::new(&mut self.tap).poll_next(cx) {
                Poll::Ready(Some(Ok(frame))) => {
                    let _ = self.frame_tx.unbounded_send(frame);
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

        loop {
            if self.sending_frame.is_some() {
                match Pin::new(&mut self.tap).poll_ready(cx) {
                    Poll::Ready(Ok(())) => {
                        if let Some(frame) = self.sending_frame.take() {
                            if let Err(e) = Pin::new(&mut self.tap).start_send(frame) {
                                panic!("writing TAP device yielded an error: {}", e);
                            }
                        }
                    }
                    Poll::Pending => {
                        break;
                    }
                    Poll::Ready(Err(e)) => {
                        panic!("completing TAP device write yielded an error: {}", e);
                    }
                }
            }

            if self.sending_frame.is_none() {
                match Pin::new(&mut self.frame_rx).poll_next(cx) {
                    Poll::Ready(Some(frame)) => {
                        self.sending_frame = Some(frame);
                        continue;
                    }
                    _ => break,
                }
            }
        }

        let mut state = mem::replace(&mut self.state, TapTaskState::Invalid);
        trace!("polling TapTask");
        loop {
            match state {
                TapTaskState::Receiving { ref exit } => {
                    trace!("state == receiving");
                    if exit.load(Ordering::Relaxed) {
                        state = TapTaskState::Dying(Timer::new(grace_period));
                        continue;
                    } else {
                        break;
                    }
                }
                TapTaskState::Dying(mut timeout) => {
                    trace!("state == dying");
                    if received_frames {
                        timeout.restart(grace_period);
                    }
                    match Pin::new(&mut timeout).poll(cx) {
                        Poll::Ready(()) => {
                            return Poll::Ready(());
                        }
                        Poll::Pending => {
                            state = TapTaskState::Dying(timeout);
                            break;
                        }
                    }
                }
                TapTaskState::Invalid => {
                    panic!("TapTask in invalid state!");
                }
            }
        }
        self.state = state;

        Poll::Pending
    }
}
