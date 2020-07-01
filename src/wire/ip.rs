use crate::network::NetworkHandle;
use crate::plug::Plug;
use crate::wire::{Ipv4Packet, Ipv4Plug, Ipv6Packet, Ipv6Plug};
use async_std::net::IpAddr;
use bytes::Bytes;
use futures::channel::mpsc::{SendError, TrySendError, UnboundedReceiver, UnboundedSender};
use futures::sink::{Sink, SinkExt};
use futures::stream::{Stream, StreamExt};
use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

#[derive(Clone, PartialEq)]
/// An IP packet.
pub enum IpPacket {
    /// IPv4
    V4(Ipv4Packet),
    /// IPv6
    V6(Ipv6Packet),
}

impl fmt::Debug for IpPacket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            IpPacket::V4(packet) => packet.fmt(f),
            IpPacket::V6(packet) => packet.fmt(f),
        }
    }
}

impl AsRef<[u8]> for IpPacket {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::V4(p) => p.as_ref(),
            Self::V6(p) => p.as_ref(),
        }
    }
}

impl IpPacket {
    /// Parse an IP packet from a byte buffer
    pub fn from_bytes(buffer: Bytes) -> IpPacket {
        match buffer[0] >> 4 {
            4 => IpPacket::V4(Ipv4Packet::from_bytes(buffer)),
            6 => IpPacket::V6(Ipv6Packet::from_bytes(buffer)),
            v => panic!("invalid IP version number: {}", v),
        }
    }

    /// Get a reference to the packet's underlying byte buffer
    pub fn as_bytes(&self) -> &Bytes {
        match self {
            IpPacket::V4(packet) => packet.as_bytes(),
            IpPacket::V6(packet) => packet.as_bytes(),
        }
    }

    /// Consume the packet and return the underlying byte buffer
    pub fn into_bytes(self) -> Bytes {
        match self {
            IpPacket::V4(packet) => packet.into_bytes(),
            IpPacket::V6(packet) => packet.into_bytes(),
        }
    }

    /// Get the packet's source IP address
    pub fn source_ip(&self) -> IpAddr {
        match self {
            IpPacket::V4(packet) => IpAddr::V4(packet.source_ip()),
            IpPacket::V6(packet) => IpAddr::V6(packet.source_ip()),
        }
    }

    /// Get the packet's destination IP address
    pub fn dest_ip(&self) -> IpAddr {
        match self {
            IpPacket::V4(packet) => IpAddr::V4(packet.dest_ip()),
            IpPacket::V6(packet) => IpAddr::V6(packet.dest_ip()),
        }
    }

    /// Unwrap this IP packet's inner IPv4Packet if possible.
    pub fn to_ipv4_packet(&self) -> Option<Ipv4Packet> {
        match self {
            IpPacket::V4(packet) => Some(packet.clone()),
            _ => None,
        }
    }

    /// Unwrap this IP packet's inner IPv6Packet if possible.
    pub fn to_ipv6_packet(&self) -> Option<Ipv6Packet> {
        match self {
            IpPacket::V6(packet) => Some(packet.clone()),
            _ => None,
        }
    }
}

#[derive(Debug)]
/// An IP plug
pub struct IpPlug {
    inner: Plug<IpPacket>,
}

impl IpPlug {
    /// Create a pair of connected plugs
    pub fn new_pair() -> (IpPlug, IpPlug) {
        let (plug_a, plug_b) = Plug::new_pair();
        let plug_a = IpPlug { inner: plug_a };
        let plug_b = IpPlug { inner: plug_b };
        (plug_a, plug_b)
    }

    /// Add latency to a connection
    pub fn with_latency(
        self,
        handle: &NetworkHandle,
        min_latency: Duration,
        mean_additional_latency: Duration,
    ) -> IpPlug {
        IpPlug {
            inner: self
                .inner
                .with_latency(handle, min_latency, mean_additional_latency),
        }
    }

    /// Add packet loss to a connection
    pub fn with_packet_loss(
        self,
        handle: &NetworkHandle,
        loss_rate: f64,
        mean_loss_duration: Duration,
    ) -> IpPlug {
        IpPlug {
            inner: self
                .inner
                .with_packet_loss(handle, loss_rate, mean_loss_duration),
        }
    }

    /// Adapt the plug to an IPv4 plug, dropping all incoming IPv6 packets.
    pub fn into_ipv4_plug(self, handle: &NetworkHandle) -> Ipv4Plug {
        let (ipv4_plug_a, ipv4_plug_b) = Ipv4Plug::new_pair();

        let (ip_tx, mut ip_rx) = self.split();
        let (ipv4_tx, mut ipv4_rx) = ipv4_plug_a.split();
        handle.spawn(Box::pin(async move {
            loop {
                match ip_rx.next().await {
                    Some(IpPacket::V4(ipv4_packet)) => {
                        ipv4_tx.unbounded_send(ipv4_packet);
                    }
                    Some(..) => {}
                    None => break,
                }
            }
        }));
        handle.spawn(Box::pin(async move {
            loop {
                match ipv4_rx.next().await {
                    Some(ipv4_packet) => {
                        let ip_packet = IpPacket::V4(ipv4_packet);
                        ip_tx.unbounded_send(ip_packet);
                    }
                    None => break,
                }
            }
        }));
        ipv4_plug_b
    }

    /// Adapt the plug to an IPv6 plug, dropping all incoming IPv6 packets.
    pub fn into_ipv6_plug(self, handle: &NetworkHandle) -> Ipv6Plug {
        let (ipv6_plug_a, ipv6_plug_b) = Ipv6Plug::new_pair();

        let (ip_tx, mut ip_rx) = self.split();
        let (ipv6_tx, mut ipv6_rx) = ipv6_plug_a.split();
        handle.spawn(Box::pin(async move {
            loop {
                match ip_rx.next().await {
                    Some(IpPacket::V6(ipv6_packet)) => {
                        ipv6_tx.unbounded_send(ipv6_packet);
                    }
                    Some(..) => {}
                    None => break,
                }
            }
        }));
        handle.spawn(Box::pin(async move {
            loop {
                match ipv6_rx.next().await {
                    Some(ipv6_packet) => {
                        let ip_packet = IpPacket::V6(ipv6_packet);
                        ip_tx.unbounded_send(ip_packet);
                    }
                    None => break,
                }
            }
        }));
        ipv6_plug_b
    }

    /// Split into sending and receiving halves
    pub fn split(self) -> (IpSender, IpReceiver) {
        let (tx, rx) = self.inner.split();
        let tx = IpSender { tx };
        let rx = IpReceiver { rx };
        (tx, rx)
    }

    /// Poll for incoming packets
    pub async fn poll_incoming(&mut self) -> Option<IpPacket> {
        self.inner.rx.next().await
    }

    /// Send a packet
    pub fn unbounded_send(&mut self, packet: IpPacket) -> Result<(), TrySendError<IpPacket>> {
        self.inner.tx.unbounded_send(packet)
    }
}

impl From<IpPlug> for Plug<IpPacket> {
    fn from(plug: IpPlug) -> Plug<IpPacket> {
        plug.inner
    }
}

impl Stream for IpPlug {
    type Item = IpPacket;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

impl Sink<IpPacket> for IpPlug {
    type Error = SendError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: IpPacket) -> Result<(), Self::Error> {
        Pin::new(&mut self.inner).start_send(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

/// A trait for types that can be converted into an `IpPlug`
pub trait IntoIpPlug {
    /// Convert into an `IpPlug`
    fn into_ip_plug(self, handle: &NetworkHandle) -> IpPlug;
}

impl<S> IntoIpPlug for S
where
    S: Stream<Item = IpPacket>,
    S: Sink<IpPacket>,
    S: Send + 'static,
{
    fn into_ip_plug(self, handle: &NetworkHandle) -> IpPlug {
        let (mut self_tx, mut self_rx) = self.split();
        let (plug_a, plug_b) = IpPlug::new_pair();
        let (mut plug_tx, mut plug_rx) = plug_a.split();
        handle.spawn(Box::pin(async move {
            while let Some(p) = self_rx.next().await {
                unwrap!(plug_tx.send(p).await.ok());
            }
        }));
        handle.spawn(Box::pin(async move {
            while let Some(p) = plug_rx.next().await {
                unwrap!(self_tx.send(p).await.ok());
            }
        }));
        plug_b
    }
}

/// The sending half of an `IpPlug`
pub struct IpSender {
    tx: UnboundedSender<IpPacket>,
}

impl IpSender {
    /// Send a packet down the wire
    pub fn unbounded_send(&self, packet: IpPacket) {
        let _ = self.tx.unbounded_send(packet);
    }
}

impl Sink<IpPacket> for IpSender {
    type Error = SendError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: IpPacket) -> Result<(), Self::Error> {
        Pin::new(&mut self.tx).start_send(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.tx).poll_close(cx)
    }
}

/// The receiving half of an `IpPlug`
pub struct IpReceiver {
    rx: UnboundedReceiver<IpPacket>,
}

impl Stream for IpReceiver {
    type Item = IpPacket;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.rx).poll_next(cx)
    }
}
