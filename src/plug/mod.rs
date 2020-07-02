use crate::network::NetworkHandle;
use futures::channel::mpsc::{self, SendError, UnboundedReceiver, UnboundedSender};
use futures::sink::Sink;
use futures::stream::Stream;
use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

mod latency;
mod packet_loss;

pub use self::latency::*;
pub use self::packet_loss::*;

#[derive(Debug)]
/// Bidirectional network plug that can be used to exchange data between two devices.
/// Anything written to the plub will be readable on the other side.
pub struct Plug<T: Unpin + fmt::Debug + 'static> {
    /// The sender
    pub tx: UnboundedSender<T>,
    /// The receiver.
    pub rx: UnboundedReceiver<T>,
}

impl<T: Unpin + fmt::Debug + Send + 'static> Plug<T> {
    /// Create a new connection connecting the two returned plugs.
    pub fn new_pair() -> (Plug<T>, Plug<T>) {
        let (a_tx, b_rx) = mpsc::unbounded();
        let (b_tx, a_rx) = mpsc::unbounded();
        let a = Plug { tx: a_tx, rx: a_rx };
        let b = Plug { tx: b_tx, rx: b_rx };
        (a, b)
    }

    /// Add latency to the end of this connection.
    ///
    /// `min_latency` is the baseline for the amount of delay added to a packet travelling on this
    /// connection. `mean_additional_latency` controls the amount of extra, random latency added to
    /// any given packet on this connection. A non-zero `mean_additional_latency` can cause packets
    /// to be re-ordered.
    pub fn with_latency(
        self,
        handle: &NetworkHandle,
        min_latency: Duration,
        mean_additional_latency: Duration,
    ) -> Plug<T> {
        let (plug_0, plug_1) = Plug::new_pair();
        Latency::spawn(handle, min_latency, mean_additional_latency, self, plug_0);
        plug_1
    }

    /// Add packet loss to the connection. Loss happens in burst, rather than on an individual
    /// packet basis. `mean_loss_duration` controls the burstiness of the loss.
    pub fn with_packet_loss(
        self,
        handle: &NetworkHandle,
        loss_rate: f64,
        mean_loss_duration: Duration,
    ) -> Plug<T> {
        let (plug_0, plug_1) = Plug::new_pair();
        PacketLoss::spawn(handle, loss_rate, mean_loss_duration, self, plug_0);
        plug_1
    }

    /// Returns sender and receiver handles used to interact with the other side of the plug.
    pub fn split(self) -> (UnboundedSender<T>, UnboundedReceiver<T>) {
        (self.tx, self.rx)
    }
}

impl<T: Unpin + fmt::Debug + 'static> Stream for Plug<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.rx).poll_next(cx)
    }
}

impl<T: Unpin + fmt::Debug + 'static> Sink<T> for Plug<T> {
    type Error = SendError;

    fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        Pin::new(&mut self.tx).start_send(item)
    }

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_ready(cx)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_close(cx)
    }
}
