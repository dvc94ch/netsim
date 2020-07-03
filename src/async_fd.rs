use smol::Async;
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};

pub type AsyncFd = Async<Fd>;

#[derive(Debug)]
pub struct Fd(RawFd);

impl Fd {
    pub fn new(fd: RawFd) -> io::Result<Self> {
        if fd < 0 {
            return Err(io::ErrorKind::InvalidInput.into());
        }

        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }

        let res = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if res < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Self(fd))
    }
}

impl AsRawFd for Fd {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.as_raw_fd());
        }
    }
}

impl Read for Fd {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        trace!("reading fd {}", self.as_raw_fd());
        let res = unsafe { libc::read(self.as_raw_fd(), buf.as_mut_ptr() as *mut _, buf.len()) };
        if res < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(res as usize)
    }
}

impl Write for Fd {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        trace!("writing fd {}", self.as_raw_fd());
        let res = unsafe { libc::write(self.as_raw_fd(), buf.as_ptr() as *mut _, buf.len()) };
        if res < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(res as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
