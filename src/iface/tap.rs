//! Contains utilites for working with virtual (TAP) network interfaces.

use crate::async_fd::AsyncFd;
use crate::iface::build::{build, IfaceBuildError, IfaceBuilder};
use crate::route::{Ipv4Route, Ipv6Route};
use crate::wire::{EtherFrame, MacAddr};
use async_std::net::{Ipv4Addr, Ipv6Addr};
use bytes::Bytes;
use futures::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, IntoSink, ReadHalf, WriteHalf};
use futures::sink::Sink;
use futures::stream::Stream;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

/// This object can be used to set the configuration options for a `EtherIface` before creating the
/// `EtherIface`
/// using `build`.
#[derive(Debug)]
pub struct EtherIfaceBuilder {
    mac_addr: MacAddr,
    builder: IfaceBuilder,
}

impl Default for EtherIfaceBuilder {
    fn default() -> EtherIfaceBuilder {
        EtherIfaceBuilder {
            mac_addr: MacAddr::random(),
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

impl EtherIfaceBuilder {
    /// Start building a new `EtherIface` with the default configuration options.
    pub fn new() -> EtherIfaceBuilder {
        Default::default()
    }

    /// Set the interface name.
    pub fn name<S: Into<String>>(mut self, name: S) -> Self {
        self.builder.name = name.into();
        self
    }

    /// Set the interface MAC address.
    pub fn mac_addr(mut self, addr: MacAddr) -> Self {
        self.mac_addr = addr;
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

    /// Consume this `EtherIfaceBuilder` and build a `UnboundEtherIface`. This creates the TAP device but does not
    /// bind it to a tokio event loop. This is useful if the event loop lives in a different thread
    /// to where you need to create the device. You can send a `UnboundEtherIface` to another thread then
    /// `bind` it to create your `EtherIface`.
    pub fn build_unbound(self) -> Result<UnboundEtherIface, IfaceBuildError> {
        let fd = build(self.builder, Some(self.mac_addr))?;

        trace!("creating TAP");

        Ok(UnboundEtherIface { fd })
    }

    /// Consume this `EtherIfaceBuilder` and build the TAP interface. The returned `EtherIface` object can be
    /// used to read/write ethernet frames from this interface.
    /// event loop which will be used for reading/writing.
    pub fn build(self) -> Result<EtherIface, IfaceBuildError> {
        Ok(self.build_unbound()?.bind())
    }
}

/// Represents a TAP device which has been built but not bound to a tokio event loop.
#[derive(Debug)]
pub struct UnboundEtherIface {
    fd: AsyncFd,
}

impl UnboundEtherIface {
    /// Bind the tap device to the event loop, creating a `EtherIface` which you can read/write ethernet
    /// frames with.
    pub fn bind(self) -> EtherIface {
        EtherIface::new(self.fd)
    }
}

/// A handle to a virtual (TAP) network interface. Can be used to read/write ethernet frames
/// directly to the device.
pub struct EtherIface {
    sink: IntoSink<WriteHalf<AsyncFd>, EtherFrame>,
    reader: ReadHalf<AsyncFd>,
}

impl EtherIface {
    fn new(fd: AsyncFd) -> Self {
        let (reader, writer) = fd.split();
        Self {
            sink: writer.into_sink(),
            reader,
        }
    }
}

impl Stream for EtherIface {
    type Item = Result<EtherFrame, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut buffer = [0u8; libc::ETH_FRAME_LEN as usize];
        match Pin::new(&mut self.reader).poll_read(cx, &mut buffer) {
            Poll::Ready(Ok(0)) => Poll::Ready(None),
            Poll::Ready(Ok(n)) => {
                let bytes = Bytes::copy_from_slice(&buffer[..n]);
                let frame = EtherFrame::from_bytes(bytes);
                info!("TAP received frame: {:?}", frame);
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
        }
    }
}

impl Sink<EtherFrame> for EtherIface {
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.sink).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: EtherFrame) -> Result<(), Self::Error> {
        info!("TAP sending frame: {:?}", item);
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

#[cfg(feature = "linux_host")]
#[cfg(test)]
mod test {
    use super::*;
    use crate::range::Ipv4Range;
    use crate::test::run_test;
    use crate::util::ipv4_addr::Ipv4AddrExt;
    use crate::util::ipv6_addr::Ipv6AddrExt;
    use crate::{iface, spawn};
    use async_std::task;
    use get_if_addrs::IfAddr;

    #[test]
    fn build_tap_correct_settings() {
        run_test(1, || {
            let spawn_complete = spawn::new_namespace(|| {
                let mac_addr = MacAddr::random();
                let ipv4_addr = Ipv4Range::local_subnet_10().random_client_addr();
                let ipv4_netmask_bits = 8 + rand::random::<u8>() % 24;
                let ipv6_addr = Ipv6Addr::random_global();
                let ipv6_netmask_bits = 64 + rand::random::<u8>() % 64;
                let name = format!("foo{:x}", rand::random::<u32>());

                let tap_builder = {
                    EtherIfaceBuilder::new()
                        .mac_addr(mac_addr)
                        .ipv4_addr(ipv4_addr, ipv4_netmask_bits)
                        .ipv6_addr(ipv6_addr, ipv6_netmask_bits)
                        .name(name.clone())
                };

                let tap = unwrap!(tap_builder.build_unbound());

                let found_mac_addr = unwrap!(iface::get_mac_addr(&name));
                assert_eq!(mac_addr, found_mac_addr);

                let mut found_v4 = false;
                let mut found_v6 = false;
                let ifaces = unwrap!(get_if_addrs::get_if_addrs());

                for iface in ifaces {
                    assert_eq!(iface.name, name);
                    match iface.addr {
                        IfAddr::V4(ref ifv4_addr) => {
                            assert!(!found_v4);
                            assert_eq!(ifv4_addr.ip, ipv4_addr);
                            assert_eq!(
                                ifv4_addr.netmask,
                                Ipv4Addr::from_netmask_bits(ipv4_netmask_bits),
                            );

                            found_v4 = true;
                        }
                        IfAddr::V6(ref ifv6_addr) => {
                            assert!(!found_v6);
                            assert_eq!(ifv6_addr.ip, ipv6_addr);
                            assert_eq!(
                                ifv6_addr.netmask,
                                Ipv6Addr::from_netmask_bits(ipv6_netmask_bits),
                            );

                            found_v6 = true;
                        }
                    }
                }
                assert!(found_v4 && found_v6);

                drop(tap);
            });
            task::block_on(spawn_complete).unwrap();
        })
    }

    #[test]
    fn build_tap_name_contains_nul() {
        run_test(1, || {
            let tap_builder = {
                EtherIfaceBuilder::new()
                    .ipv4_addr(Ipv4Addr::random_global(), 0)
                    .name("hello\0")
            };
            let res = tap_builder.build_unbound();
            match res {
                Err(IfaceBuildError::NameContainsNul) => (),
                x => panic!("unexpected result: {:?}", x),
            }
        })
    }

    #[test]
    fn build_tap_duplicate_name() {
        run_test(3, || {
            let spawn_complete = spawn::new_namespace(|| {
                let tap_builder = {
                    EtherIfaceBuilder::new()
                        .ipv4_addr(Ipv4Addr::random_global(), 0)
                        .name("hello")
                };
                trace!("build_tap_duplicate_name: building first interface");
                let _tap = unwrap!(tap_builder.build_unbound());

                let tap_builder = {
                    EtherIfaceBuilder::new()
                        .ipv4_addr(Ipv4Addr::random_global(), 0)
                        .name("hello")
                };
                trace!("build_tap_duplicate_name: building second interface");
                match tap_builder.build_unbound() {
                    Err(IfaceBuildError::InterfaceAlreadyExists) => (),
                    res => panic!("unexpected result: {:?}", res),
                }
                trace!("build_tap_duplicate_name: done");
            });
            task::block_on(spawn_complete).unwrap();
        });
    }

    #[test]
    fn build_tap_permission_denied() {
        run_test(3, || {
            let spawn_complete = spawn::new_namespace(|| {
                unwrap!(unwrap!(capabilities::Capabilities::new()).apply());

                let tap_builder = EtherIfaceBuilder::new();
                match tap_builder.build_unbound() {
                    Err(IfaceBuildError::CreateIfacePermissionDenied) => (),
                    res => panic!("unexpected result: {:?}", res),
                }
            });
            task::block_on(spawn_complete).unwrap();
        })
    }
}
