use crate::wire::IpPacket;
use async_std::fs::File;
use async_std::prelude::*;
use byteorder::{NativeEndian, WriteBytesExt};
use bytes::Bytes;
use futures::io::AsyncWrite;
use futures::sink::Sink;
use std::io::Cursor;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{io, mem};

/// A sink for IP packets which writes the packets to a pcap file.
pub struct IpLog {
    file: File,
    state: LogState,
}

enum LogState {
    WritingHeader {
        header: [u8; 16],
        bytes: Bytes,
        written: usize,
    },
    WritingPayload {
        bytes: Bytes,
        written: usize,
    },
    Ready,
    Invalid,
}

impl IpLog {
    /// Create a new log file.
    pub async fn new(path: &Path) -> Result<IpLog, io::Error> {
        const MAGIC: u32 = 0xa1b2_3c4d;
        const VERSION_MAJOR: u16 = 2;
        const VERSION_MINOR: u16 = 4;
        const LINKTYPE_RAW: u32 = 101;

        let mut file = File::open(path).await?;
        let mut header = [0u8; 24];
        {
            let mut cursor = Cursor::new(&mut header[..]);
            unwrap!(cursor.write_u32::<NativeEndian>(MAGIC));
            unwrap!(cursor.write_u16::<NativeEndian>(VERSION_MAJOR));
            unwrap!(cursor.write_u16::<NativeEndian>(VERSION_MINOR));
            unwrap!(cursor.write_i32::<NativeEndian>(0));
            unwrap!(cursor.write_u32::<NativeEndian>(0));
            unwrap!(cursor.write_u32::<NativeEndian>(65_536));
            unwrap!(cursor.write_u32::<NativeEndian>(LINKTYPE_RAW));
        }
        file.write_all(&header[..]).await?;
        Ok(IpLog {
            file,
            state: LogState::Ready,
        })
    }
}

impl Sink<IpPacket> for IpLog {
    type Error = io::Error;

    fn start_send(mut self: Pin<&mut Self>, packet: IpPacket) -> Result<(), Self::Error> {
        let state = mem::replace(&mut self.state, LogState::Invalid);
        match state {
            LogState::Ready => {
                let bytes = packet.into_bytes();
                let now = unwrap!(SystemTime::now().duration_since(UNIX_EPOCH));
                let mut header = [0u8; 16];
                {
                    let mut cursor = Cursor::new(&mut header[..]);
                    unwrap!(cursor.write_u32::<NativeEndian>(now.as_secs() as u32));
                    unwrap!(cursor.write_u32::<NativeEndian>(now.subsec_nanos()));
                    unwrap!(cursor.write_u32::<NativeEndian>(bytes.len() as u32));
                    unwrap!(cursor.write_u32::<NativeEndian>(bytes.len() as u32));
                }
                self.state = LogState::WritingHeader {
                    header,
                    bytes,
                    written: 0,
                };
                Ok(())
            }
            LogState::Invalid => panic!("invalid LogState"),
            _ => {
                self.state = state;
                Err(io::Error::new(io::ErrorKind::WouldBlock, "would block"))
            }
        }
    }

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        let mut state = mem::replace(&mut self.state, LogState::Invalid);
        loop {
            match state {
                LogState::Invalid => panic!("invalid LogState"),
                LogState::Ready => {
                    self.state = state;
                    return Poll::Ready(Ok(()));
                }
                LogState::WritingHeader {
                    header,
                    bytes,
                    written,
                } => match Pin::new(&mut self.file).poll_write(cx, &header[written..]) {
                    Poll::Ready(Ok(n)) => {
                        let new_written = written + n;
                        if new_written == header.len() {
                            state = LogState::WritingPayload { bytes, written: 0 };
                        } else {
                            state = LogState::WritingHeader {
                                header,
                                bytes,
                                written: new_written,
                            };
                        }
                        continue;
                    }
                    Poll::Pending => {
                        self.state = LogState::WritingHeader {
                            header,
                            bytes,
                            written,
                        };
                        return Poll::Pending;
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                },
                LogState::WritingPayload { bytes, written } => {
                    match Pin::new(&mut self.file).poll_write(cx, &bytes[written..]) {
                        Poll::Ready(Ok(n)) => {
                            let new_written = written + n;
                            if new_written == bytes.len() {
                                state = LogState::Ready;
                            } else {
                                state = LogState::WritingPayload {
                                    bytes,
                                    written: new_written,
                                };
                            }
                            continue;
                        }
                        Poll::Pending => {
                            self.state = LogState::WritingPayload { bytes, written };
                            return Poll::Pending;
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    }
                }
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.file).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.file).poll_close(cx)
    }
}
