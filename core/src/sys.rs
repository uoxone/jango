//! Linux sockets and epoll.
//!
//! Unsafe stays in this file. Callers get `io::Result`
//! and never touch a raw file descriptor helper.

use std::io::{self, ErrorKind};
use std::net::SocketAddrV4;

const AF_INET: i32 = 2;
const SOCK_STREAM: i32 = 1;
const SOCK_NONBLOCK: i32 = 2048;
const SOCK_CLOEXEC: i32 = 524288;
const SOL_SOCKET: i32 = 1;
const SO_REUSEADDR: i32 = 2;
const SO_REUSEPORT: i32 = 15;
const IPPROTO_TCP: i32 = 6;
const TCP_NODELAY: i32 = 1;
pub(crate) const EPOLLIN: u32 = 1;
const EPOLLRDHUP: u32 = 8192;
pub(crate) const EPOLLERR: u32 = 8;
pub(crate) const EPOLLHUP: u32 = 16;
const EPOLL_CTL_ADD: i32 = 1;
const EPOLL_CTL_DEL: i32 = 2;
const EPOLL_CLOEXEC: i32 = 524288;
const POLLOUT: i16 = 4;

// Linux x86_64 packs this struct to 12 bytes.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub(crate) struct EpollEvent {
    pub events: u32,
    pub data: u64,
}

#[repr(C)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: u32,
    sin_zero: [u8; 8],
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[repr(C)]
struct Iovec {
    iov_base: *const u8,
    iov_len: usize,
}

mod ffi {
    use super::{EpollEvent, Iovec, PollFd, SockAddrIn};

    unsafe extern "C" {
        pub fn socket(domain: i32, ty: i32, protocol: i32) -> i32;
        pub fn setsockopt(fd: i32, level: i32, name: i32, value: *const i32, len: u32) -> i32;
        pub fn bind(fd: i32, addr: *const SockAddrIn, len: u32) -> i32;
        pub fn listen(fd: i32, backlog: i32) -> i32;
        pub fn accept4(fd: i32, addr: *mut (), len: *mut u32, flags: i32) -> i32;
        pub fn epoll_create1(flags: i32) -> i32;
        pub fn epoll_ctl(ep: i32, op: i32, fd: i32, event: *mut EpollEvent) -> i32;
        pub fn epoll_wait(ep: i32, events: *mut EpollEvent, max: i32, timeout: i32) -> i32;
        pub fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
        pub fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
        pub fn writev(fd: i32, iov: *const Iovec, count: i32) -> isize;
        pub fn close(fd: i32) -> i32;
    }
}

// -----------------------------------------------
// Listen socket
// SO_REUSEPORT lets every core bind the same port
// and accept without a shared lock.
// -----------------------------------------------

pub(crate) fn listen_socket(addr: SocketAddrV4) -> io::Result<i32> {
    let fd = unsafe { ffi::socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    if let Err(err) = configure(fd, addr) {
        let _ = close_fd(fd);
        return Err(err);
    }
    Ok(fd)
}

fn configure(fd: i32, addr: SocketAddrV4) -> io::Result<()> {
    set_int(fd, SOL_SOCKET, SO_REUSEADDR, 1)?;
    set_int(fd, SOL_SOCKET, SO_REUSEPORT, 1)?;
    // Octets are stored in network order inside sockaddr_in.
    let sock = SockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: addr.port().to_be(),
        sin_addr: u32::from_ne_bytes(addr.ip().octets()),
        sin_zero: [0; 8],
    };
    let bound = unsafe { ffi::bind(fd, &sock, std::mem::size_of::<SockAddrIn>() as u32) };
    if bound != 0 {
        return Err(io::Error::last_os_error());
    }
    let listening = unsafe { ffi::listen(fd, 4096) };
    if listening != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_int(fd: i32, level: i32, name: i32, value: i32) -> io::Result<()> {
    let rc = unsafe { ffi::setsockopt(fd, level, name, &value, 4) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(crate) fn nodelay(fd: i32) -> io::Result<()> {
    set_int(fd, IPPROTO_TCP, TCP_NODELAY, 1)
}

// -----------------------------------------------
// Accept, epoll, read, write
// Edge notes: the listen socket is non-blocking.
// Accept stops on EAGAIN. Writes wait with poll
// only when the kernel buffer is full.
// -----------------------------------------------

pub(crate) const LISTEN: u64 = u64::MAX;
pub(crate) const INTEREST: u32 = EPOLLIN | EPOLLRDHUP;
pub(crate) const PEER_GONE: u32 = EPOLLERR | EPOLLHUP | EPOLLRDHUP;

pub(crate) fn accept(fd: i32) -> io::Result<i32> {
    loop {
        let got = unsafe {
            ffi::accept4(
                fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                SOCK_NONBLOCK | SOCK_CLOEXEC,
            )
        };
        if got >= 0 {
            return Ok(got);
        }
        let err = io::Error::last_os_error();
        if err.kind() != ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

pub(crate) fn epoll_create() -> io::Result<i32> {
    let fd = unsafe { ffi::epoll_create1(EPOLL_CLOEXEC) };
    if fd >= 0 {
        Ok(fd)
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(crate) fn epoll_add(ep: i32, fd: i32, data: u64) -> io::Result<()> {
    let mut event = EpollEvent {
        events: INTEREST,
        data,
    };
    ctl(ep, EPOLL_CTL_ADD, fd, &mut event)
}

pub(crate) fn epoll_del(ep: i32, fd: i32) -> io::Result<()> {
    let mut event = EpollEvent { events: 0, data: 0 };
    ctl(ep, EPOLL_CTL_DEL, fd, &mut event)
}

fn ctl(ep: i32, op: i32, fd: i32, event: &mut EpollEvent) -> io::Result<()> {
    let rc = unsafe { ffi::epoll_ctl(ep, op, fd, event) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(crate) fn epoll_wait(ep: i32, events: &mut [EpollEvent]) -> io::Result<usize> {
    loop {
        let n = unsafe { ffi::epoll_wait(ep, events.as_mut_ptr(), events.len() as i32, -1) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let err = io::Error::last_os_error();
        if err.kind() != ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

/// Read into the spare capacity of `buf`.
///
/// Returns 0 when the peer closed the connection.
pub(crate) fn read_append(fd: i32, buf: &mut Vec<u8>) -> io::Result<usize> {
    if buf.capacity() - buf.len() < 1024 {
        buf.reserve(8192);
    }
    let start = buf.len();
    let space = buf.capacity() - start;
    // SAFETY: `start` is `buf.len()`, so the spare capacity is allocated
    // and belongs to this vector. `set_len` runs only for bytes `read`
    // actually wrote.
    let n = unsafe { ffi::read(fd, buf.as_mut_ptr().add(start), space) };
    if n > 0 {
        unsafe { buf.set_len(start + n as usize) };
        Ok(n as usize)
    } else if n == 0 {
        Ok(0)
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(crate) fn write_two(fd: i32, mut first: &[u8], mut second: &[u8]) -> io::Result<()> {
    while !first.is_empty() || !second.is_empty() {
        let iov = [
            Iovec {
                iov_base: first.as_ptr(),
                iov_len: first.len(),
            },
            Iovec {
                iov_base: second.as_ptr(),
                iov_len: second.len(),
            },
        ];
        let (ptr, count) = if first.is_empty() {
            (&iov[1] as *const Iovec, 1)
        } else if second.is_empty() {
            (iov.as_ptr(), 1)
        } else {
            (iov.as_ptr(), 2)
        };
        let n = unsafe { ffi::writev(fd, ptr, count) };
        if n > 0 {
            let mut left = n as usize;
            let take = left.min(first.len());
            first = &first[take..];
            left -= take;
            let take = left.min(second.len());
            second = &second[take..];
        } else if n == 0 {
            return Err(io::Error::new(ErrorKind::WriteZero, "socket closed"));
        } else {
            let err = io::Error::last_os_error();
            if err.kind() == ErrorKind::Interrupted {
                continue;
            }
            if err.kind() == ErrorKind::WouldBlock {
                wait_write(fd)?;
                continue;
            }
            return Err(err);
        }
    }
    Ok(())
}

fn wait_write(fd: i32) -> io::Result<()> {
    let mut pollfd = PollFd {
        fd,
        events: POLLOUT,
        revents: 0,
    };
    loop {
        let n = unsafe { ffi::poll(&mut pollfd, 1, 5_000) };
        if n > 0 {
            return Ok(());
        }
        if n == 0 {
            return Err(io::Error::new(ErrorKind::TimedOut, "write timed out"));
        }
        let err = io::Error::last_os_error();
        if err.kind() != ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

pub(crate) fn close_fd(fd: i32) -> io::Result<()> {
    let rc = unsafe { ffi::close(fd) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
#[path = "../tests/sys.rs"]
mod tests;
