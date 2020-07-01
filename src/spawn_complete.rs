//! This module implements futures to wait for task completion.

use crate::process_handle::ProcessHandle;
use futures::channel::oneshot;
use futures::future::Future;
use std::any::Any;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::thread;

/// A handle to the spawned network-isolated thread. Implements `Future` so that you can wait for
/// the thread to complete.
pub struct SpawnComplete<R> {
    ret_rx: oneshot::Receiver<thread::Result<R>>,
    process_handle: Option<ProcessHandle>,
}

impl<R> SpawnComplete<R> {
    /// Constructs `SpawnComplete` that waits for a completion signal and until a given process stops.
    pub fn from_parts(
        ret_rx: oneshot::Receiver<thread::Result<R>>,
        process_handle: ProcessHandle,
    ) -> Self {
        Self {
            ret_rx,
            process_handle: Some(process_handle),
        }
    }

    /// Constructs `SpawnComplete` that waits for a completion signal.
    pub fn from_receiver(ret_rx: oneshot::Receiver<thread::Result<R>>) -> Self {
        Self {
            ret_rx,
            process_handle: None,
        }
    }
}

impl<R> Future for SpawnComplete<R> {
    type Output = Result<R, Box<dyn Any + Send + 'static>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match Pin::new(&mut self.ret_rx).poll(cx) {
            Poll::Ready(Ok(res)) => {
                if let Some(mut process_handle) = self.process_handle.take() {
                    process_handle.busy_wait_for_exit();
                }
                Poll::Ready(res)
            }
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(oneshot::Canceled)) => {
                let holds_process = self.process_handle.is_some();
                panic!(
                    "thread destroyed without sending response!? {}",
                    holds_process
                );
            }
        }
    }
}
