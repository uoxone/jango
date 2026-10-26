//! HTTP server.
//!
//! One thread per core. Each thread has its own
//! listen socket (SO_REUSEPORT) and its own epoll
//! loop, so cores do not share a connection lock.

#![forbid(unsafe_code)]

use std::io::{self, ErrorKind, Seek, SeekFrom};
use std::net::{SocketAddr, SocketAddrV4, ToSocketAddrs};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::http::{reason, Request, Response};
use crate::parse::{self, Parse};
use crate::router::Router;
use crate::sys::{self, EpollEvent};

const MAX_CONN: usize = 8192;

// -----------------------------------------------
// run
// Resolves the address, starts one worker per core,
// and waits there. A view runs on the worker that
// accepted the connection, so keep views short.
// -----------------------------------------------

/// Listen on `addr` and serve `router` until the process stops.
///
/// `addr` is an IPv4 socket such as `127.0.0.1:8000`.
/// `localhost:8000` is accepted when it resolves to IPv4.
pub fn run(addr: &str, router: Router) -> io::Result<()> {
    let addr = resolve(addr)?;
    let workers = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let router = Arc::new(router);
    let (ready_tx, ready_rx) = mpsc::channel();
    let (alive_tx, alive_rx) = mpsc::channel::<()>();

    for id in 0..workers {
        let ready_tx = ready_tx.clone();
        let alive_tx = alive_tx.clone();
        let router = Arc::clone(&router);
        thread::Builder::new()
            .name(format!("ongo-{id}"))
            .spawn(move || {
                let _stay = alive_tx;
                worker(addr, &router, ready_tx);
            })?;
    }
    drop(ready_tx);
    drop(alive_tx);

    let mut up = 0;
    let mut failure = None;
    for _ in 0..workers {
        match ready_rx.recv() {
            Ok(Ok(())) => up += 1,
            Ok(Err(err)) => failure = Some(err),
            Err(_) => break,
        }
    }
    if up == 0 {
        return Err(failure.unwrap_or_else(|| io::Error::other("could not bind")));
    }

    println!("ongo  http://{addr}  ({up} workers)");
    let _ = alive_rx.recv();
    Ok(())
}

/// The first IPv4 address. An IPv6-only name is an error.
fn resolve(addr: &str) -> io::Result<SocketAddrV4> {
    let mut saw_v6 = false;
    for found in addr.to_socket_addrs()? {
        match found {
            SocketAddr::V4(v4) => return Ok(v4),
            SocketAddr::V6(_) => saw_v6 = true,
        }
    }
    let message = if saw_v6 {
        "ongo listens on IPv4; use an address such as 127.0.0.1:8000"
    } else {
        "could not resolve the listen address"
    };
    Err(io::Error::new(ErrorKind::InvalidInput, message))
}

/// One core. It owns a listen socket and an epoll loop.
fn worker(addr: SocketAddrV4, router: &Router, ready: mpsc::Sender<io::Result<()>>) {
    let listen = match sys::listen_socket(addr) {
        Ok(fd) => fd,
        Err(err) => {
            let _ = ready.send(Err(err));
            return;
        }
    };
    let ep = match sys::epoll_create() {
        Ok(fd) => fd,
        Err(err) => {
            let _ = sys::close_fd(listen);
            let _ = ready.send(Err(err));
            return;
        }
    };
    if let Err(err) = sys::epoll_add(ep, listen, sys::LISTEN) {
        let _ = sys::close_fd(ep);
        let _ = sys::close_fd(listen);
        let _ = ready.send(Err(err));
        return;
    }
    let _ = ready.send(Ok(()));

    let mut table = Table::default();
    let mut events = vec![EpollEvent { events: 0, data: 0 }; 1024];
    loop {
        let ready_count = match sys::epoll_wait(ep, &mut events) {
            Ok(count) => count,
            Err(err) => {
                eprintln!("ongo: {err}");
                break;
            }
        };
        for event in events.iter().take(ready_count) {
            // Copy the packed fields. Do not borrow them.
            let flags = event.events;
            let data = event.data;
            if data == sys::LISTEN {
                accept_all(listen, ep, &mut table);
                continue;
            }
            let id = data as usize;
            let gone = flags & sys::PEER_GONE != 0 && flags & sys::EPOLLIN == 0;
            if gone || !drive(&mut table, id, router) {
                drop_conn(&mut table, ep, id);
            }
        }
    }
    let _ = sys::close_fd(ep);
    let _ = sys::close_fd(listen);
}

/// Accept until the kernel says the queue is empty.
fn accept_all(listen: i32, ep: i32, table: &mut Table) {
    loop {
        match sys::accept(listen) {
            Ok(fd) => {
                if table.len >= MAX_CONN {
                    let _ = sys::close_fd(fd);
                    continue;
                }
                if sys::nodelay(fd).is_err() {
                    let _ = sys::close_fd(fd);
                    continue;
                }
                let id = table.insert(fd);
                if sys::epoll_add(ep, fd, id as u64).is_err() {
                    drop_conn(table, ep, id);
                }
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => break,
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(err) => {
                eprintln!("ongo: accept: {err}");
                thread::sleep(Duration::from_millis(1));
                break;
            }
        }
    }
}

/// Read and answer on this connection. `false` means close it.
fn drive(table: &mut Table, id: usize, router: &Router) -> bool {
    let Some(conn) = table.get_mut(id) else {
        return false;
    };
    loop {
        if conn.buf.len() > parse::MAX_HEADER + parse::MAX_BODY {
            return false;
        }
        match parse::parse(&conn.buf) {
            // Read only when the buffer is short a full request.
            // A peer that sends the request and closes is still answered.
            Parse::Need => match read_some(conn) {
                Read::Data => continue,
                Read::Empty => return true,
                Read::Closed | Read::Err => return false,
            },
            Parse::Fail(status) => {
                let response = Response::status(status, reason(status));
                let _ = send(conn.fd, response, false, true);
                return false;
            }
            Parse::Hit(hit) => {
                let send_body = hit.method != crate::http::Method::Head;
                let keep = hit.keep_alive;
                let total = hit.total;
                let response = dispatch(router, &hit);
                if send(conn.fd, response, keep, send_body).is_err() {
                    return false;
                }
                conn.buf.copy_within(total.., 0);
                conn.buf.truncate(conn.buf.len() - total);
                if !keep {
                    return false;
                }
            }
        }
    }
}

/// What one non-blocking read did.
enum Read {
    /// Bytes were appended to the connection buffer.
    Data,
    /// The socket has nothing more right now.
    Empty,
    /// The peer closed the connection.
    Closed,
    /// The read failed.
    Err,
}

/// One read from the socket into the connection buffer.
fn read_some(conn: &mut Conn) -> Read {
    loop {
        match sys::read_append(conn.fd, &mut conn.buf) {
            Ok(0) => return Read::Closed,
            Ok(_) => return Read::Data,
            Err(err) if err.kind() == ErrorKind::WouldBlock => return Read::Empty,
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Read::Err,
        }
    }
}

/// Run the view. A panic in the view becomes a 500.
fn dispatch(router: &Router, hit: &parse::Parsed<'_>) -> Response {
    let mut req = Request::new(
        hit.method,
        hit.path,
        hit.query,
        &hit.headers[..hit.header_count],
        hit.body,
    );
    match catch_unwind(AssertUnwindSafe(|| router.call(&mut req))) {
        Ok(response) => response,
        Err(_) => Response::status(500, "Internal Server Error"),
    }
}

/// Write headers, then the memory body or a static file.
///
/// `send_body` is false for `HEAD`: the length is still sent.
fn send(fd: i32, mut response: Response, keep_alive: bool, send_body: bool) -> io::Result<()> {
    let len = response.output_len();
    let opened = match response.file.take() {
        Some(mut file) if send_body => Some(open_file(&mut file)?),
        _ => None,
    };
    let body: &[u8] = if opened.is_none() && send_body {
        &response.body
    } else {
        b""
    };
    let mut stack = [0u8; 1024];
    if let Some(header_len) = write_header(&mut stack, &response, len, keep_alive) {
        sys::write_two(fd, &stack[..header_len], body)?;
    } else {
        let mut heap = vec![0u8; response_header_len(&response, len)];
        let n = write_header(&mut heap, &response, len, keep_alive).expect("header buffer");
        heap.truncate(n);
        sys::write_two(fd, &heap, body)?;
    }
    if let Some(mut file) = opened {
        write_file(fd, &mut file, len)?;
    }
    Ok(())
}

// -----------------------------------------------
// Static body
// Headers go out first, with Content-Length.
// The file follows in 64 KiB pieces, from the
// byte the range asked for.
// -----------------------------------------------

/// Use the file already opened for a static response, or open `path`.
///
/// A range starts at `file.start`. The common case starts at byte 0.
fn open_file(file: &mut crate::http::FileBody) -> io::Result<std::fs::File> {
    let mut opened = match file.opened.take() {
        Some(opened) => opened,
        None => std::fs::File::open(&file.path)?,
    };
    if file.start > 0 {
        opened.seek(SeekFrom::Start(file.start))?;
    }
    Ok(opened)
}

/// Copy `left` bytes from the file to the socket, 64 KiB at a time.
fn write_file(fd: i32, file: &mut std::fs::File, mut left: u64) -> io::Result<()> {
    use std::io::Read;

    let mut buf = [0u8; 65536];
    while left > 0 {
        let want = left.min(buf.len() as u64) as usize;
        let n = file.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "static file ended early",
            ));
        }
        sys::write_two(fd, &buf[..n], b"")?;
        left -= n as u64;
    }
    Ok(())
}

/// Bytes the header can need, so the heap buffer is large enough.
fn response_header_len(response: &Response, body_len: u64) -> usize {
    160 + response.content_type.len()
        + digits(body_len)
        + response
            .location
            .as_ref()
            .map(|loc| loc.len() + 12)
            .unwrap_or(0)
        + if response.accept_ranges { 22 } else { 0 }
        + response
            .content_range
            .as_ref()
            .map(|range| range.len() + 17)
            .unwrap_or(0)
}

/// Write the status line and headers into `buf`. `None` if `buf` is short.
fn write_header(buf: &mut [u8], response: &Response, len: u64, keep: bool) -> Option<usize> {
    let mut out = Out { buf, n: 0 };
    out.bytes(b"HTTP/1.1 ")?;
    out.status(response.status)?;
    out.byte(b' ')?;
    out.bytes(reason(response.status).as_bytes())?;
    out.bytes(b"\r\nContent-Type: ")?;
    out.bytes(response.content_type.as_bytes())?;
    out.bytes(b"\r\nContent-Length: ")?;
    out.u64(len)?;
    if response.accept_ranges {
        out.bytes(b"\r\nAccept-Ranges: bytes")?;
    }
    if let Some(range) = &response.content_range {
        out.bytes(b"\r\nContent-Range: ")?;
        out.bytes(range.as_bytes())?;
    }
    out.bytes(if keep {
        b"\r\nConnection: keep-alive\r\nServer: ongo\r\n"
    } else {
        b"\r\nConnection: close\r\nServer: ongo\r\n"
    })?;
    if let Some(location) = &response.location {
        out.bytes(b"Location: ")?;
        out.bytes(location.as_bytes())?;
        out.bytes(b"\r\n")?;
    }
    out.bytes(b"\r\n")?;
    Some(out.n)
}

struct Out<'a> {
    buf: &'a mut [u8],
    n: usize,
}

impl Out<'_> {
    fn bytes(&mut self, data: &[u8]) -> Option<()> {
        let next = self.n.checked_add(data.len())?;
        if next > self.buf.len() {
            return None;
        }
        self.buf[self.n..next].copy_from_slice(data);
        self.n = next;
        Some(())
    }

    fn byte(&mut self, data: u8) -> Option<()> {
        self.bytes(&[data])
    }

    fn status(&mut self, code: u16) -> Option<()> {
        self.byte(b'0' + (code / 100) as u8)?;
        self.byte(b'0' + ((code / 10) % 10) as u8)?;
        self.byte(b'0' + (code % 10) as u8)
    }

    fn u64(&mut self, mut value: u64) -> Option<()> {
        let mut tmp = [0u8; 20];
        let mut at = 20;
        loop {
            at -= 1;
            tmp[at] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        self.bytes(&tmp[at..])
    }
}

/// How many characters `value` needs in `Content-Length`.
fn digits(mut value: u64) -> usize {
    let mut count = 1;
    while value >= 10 {
        value /= 10;
        count += 1;
    }
    count
}

// -----------------------------------------------
// Connection table
// Slots are reused so a busy server does not
// allocate a new buffer for every accept.
// -----------------------------------------------

struct Conn {
    fd: i32,
    buf: Vec<u8>,
}

#[derive(Default)]
struct Table {
    conns: Vec<Option<Conn>>,
    free: Vec<usize>,
    len: usize,
}

impl Table {
    fn insert(&mut self, fd: i32) -> usize {
        let conn = match self.free.pop() {
            Some(id) => {
                let mut conn = self.conns[id].take().unwrap_or(Conn {
                    fd,
                    buf: Vec::with_capacity(8192),
                });
                conn.fd = fd;
                conn.buf.clear();
                self.conns[id] = Some(conn);
                self.len += 1;
                return id;
            }
            None => Conn {
                fd,
                buf: Vec::with_capacity(8192),
            },
        };
        self.conns.push(Some(conn));
        self.len += 1;
        self.conns.len() - 1
    }

    fn get_mut(&mut self, id: usize) -> Option<&mut Conn> {
        self.conns.get_mut(id)?.as_mut()
    }
}

/// Close the socket and keep the read buffer for the next connection.
fn drop_conn(table: &mut Table, ep: i32, id: usize) {
    let Some(slot) = table.conns.get_mut(id) else {
        return;
    };
    let Some(conn) = slot.take() else {
        return;
    };
    let _ = sys::epoll_del(ep, conn.fd);
    let _ = sys::close_fd(conn.fd);
    table.conns[id] = Some(Conn {
        fd: -1,
        buf: conn.buf,
    });
    table.free.push(id);
    table.len = table.len.saturating_sub(1);
}

#[cfg(test)]
#[path = "../tests/server.rs"]
mod tests;
