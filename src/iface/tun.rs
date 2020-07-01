//! Contains utilites for working with virtual (TUN) network interfaces.

use crate::async_fd::AsyncFd;
use crate::iface::build::{build, IfaceBuildError, IfaceBuilder};
use crate::route::{Ipv4Route, Ipv6Route};
use crate::wire::IpPacket;
use async_std::net::{Ipv4Addr, Ipv6Addr};
use bytes::Bytes;
use futures::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, IntoSink, ReadHalf, WriteHalf};
use futures::sink::Sink;
use futures::stream::Stream;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

/// This object can be used to set the configuration options for a `IpIface` before creating the
/// `IpIface`
/// using `build`.
#[derive(Debug)]
pub struct IpIfaceBuilder {
    builder: IfaceBuilder,
}

impl Default for IpIfaceBuilder {
    fn default() -> IpIfaceBuilder {
        IpIfaceBuilder {
            builder: IfaceBuilder {
                name: String::from("netsim"),
                ipv4_addr: None,
                ipv6_addr: None,
                ipv4_routes: Vec::new(),
                ipv6_routes: Vec::new(),
            },
        }
    }
}

impl IpIfaceBuilder {
    /// Start building a new `IpIface` with the default configuration options.
    pub fn new() -> IpIfaceBuilder {
        Default::default()
    }

    /// Set the interface name.
    pub fn name<S: Into<String>>(mut self, name: S) -> Self {
        self.builder.name = name.into();
        self
    }

    /// Set the interface's IPv4 address and netmask
    pub fn ipv4_addr(mut self, addr: Ipv4Addr, netmask_bits: u8) -> Self {
        self.builder.ipv4_addr = Some((addr, netmask_bits));
        self
    }

    /// Set the interface's IPv6 address and netmask
    pub fn ipv6_addr(mut self, addr: Ipv6Addr, netmask_bits: u8) -> Self {
        self.builder.ipv6_addr = Some((addr, netmask_bits));
        self
    }

    /// Add an IPv4 route to the interface
    pub fn ipv4_route(mut self, route: Ipv4Route) -> Self {
        self.builder.ipv4_routes.push(route);
        self
    }

    /// Add an IPv6 route to the interface
    pub fn ipv6_route(mut self, route: Ipv6Route) -> Self {
        self.builder.ipv6_routes.push(route);
        self
    }

    /// Consume this `IpIfaceBuilder` and build a `UnboundIpIface`. This creates the TUN device but does not
    /// bind it to a tokio event loop. This is useful if the event loop lives in a different thread
    /// to where you need to create the device. You can send a `UnboundIpIface` to another thread then
    /// `bind` it to create your `IpIface`.
    pub fn build_unbound(self) -> Result<UnboundIpIface, IfaceBuildError> {
        let fd = build(self.builder, None)?;

        trace!("creating TUN");

        Ok(UnboundIpIface { fd })
    }

    /// Consume this `IpIfaceBuilder` and build the TUN interface. The returned `IpIface` object can be
    /// used to read/write ethernet frames from this interface. `handle` is a handle to a tokio
    /// event loop which will be used for reading/writing.
    pub fn build(self) -> Result<IpIface, IfaceBuildError> {
        Ok(self.build_unbound()?.bind())
    }
}

/// Represents a TUN device which has been built but not bound to a tokio event loop.
#[derive(Debug)]
pub struct UnboundIpIface {
    fd: AsyncFd,
}

impl UnboundIpIface {
    /// Bind the tap device to the event loop, creating a `IpIface` which you can read/write ethernet
    /// frames with.
    pub fn bind(self) -> IpIface {
        IpIface::new(self.fd)
    }
}

/// A handle to a virtual (TUN) network interface. Can be used to read/write ethernet frames
/// directly to the device.
pub struct IpIface {
    sink: IntoSink<WriteHalf<AsyncFd>, IpPacket>,
    reader: ReadHalf<AsyncFd>,
}

impl IpIface {
    fn new(fd: AsyncFd) -> Self {
        let (reader, writer) = fd.split();
        Self {
            sink: writer.into_sink(),
            reader,
        }
    }
}

impl Stream for IpIface {
    type Item = Result<IpPacket, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut buffer = [0u8; libc::ETH_FRAME_LEN as usize];
        match Pin::new(&mut self.reader).poll_read(cx, &mut buffer) {
            Poll::Ready(Ok(0)) => Poll::Ready(None),
            Poll::Ready(Ok(n)) => {
                let bytes = Bytes::copy_from_slice(&buffer[..n]);
                let frame = IpPacket::from_bytes(bytes);
                info!("TUN received packet: {:?}", frame);
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
        }
    }
}

impl Sink<IpPacket> for IpIface {
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sink).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: IpPacket) -> Result<(), Self::Error> {
        info!("TUN sending packet: {:?}", item);
        Pin::new(&mut self.sink).start_send(item)?;
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sink).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sink).poll_close(cx)
    }
}
