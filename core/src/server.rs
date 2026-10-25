//! HTTP server.
//!
//! One thread per core. Each thread has its own
//! listen socket (SO_REUSEPORT) and its own epoll
//! loop, so cores do not share a connection lock.

#![forbid(unsafe_code)]

use std::io::{self, ErrorKind};
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
            .name(format!("jango-{id}"))
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

    println!("jango  http://{addr}  ({up} workers)");
    let _ = alive_rx.recv();
    Ok(())
}

fn resolve(addr: &str) -> io::Result<SocketAddrV4> {
    let mut saw_v6 = false;
    for found in addr.to_socket_addrs()? {
        match found {
            SocketAddr::V4(v4) => return Ok(v4),
            SocketAddr::V6(_) => saw_v6 = true,
        }
    }
    let message = if saw_v6 {
        "jango listens on IPv4; use an address such as 127.0.0.1:8000"
    } else {
        "could not resolve the listen address"
    };
    Err(io::Error::new(ErrorKind::InvalidInput, message))
}

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
                eprintln!("jango: {err}");
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
                eprintln!("jango: accept: {err}");
                thread::sleep(Duration::from_millis(1));
                break;
            }
        }
    }
}

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
                let _ = send(conn.fd, &response, false, true);
                return false;
            }
            Parse::Hit(hit) => {
                let send_body = hit.method != crate::http::Method::Head;
                let keep = hit.keep_alive;
                let total = hit.total;
                let response = dispatch(router, &hit);
                if send(conn.fd, &response, keep, send_body).is_err() {
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

enum Read {
    Data,
    Empty,
    Closed,
    Err,
}

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

fn send(fd: i32, response: &Response, keep_alive: bool, send_body: bool) -> io::Result<()> {
    let len = response.body.len();
    let body: &[u8] = if send_body { &response.body } else { b"" };
    let mut stack = [0u8; 1024];
    if let Some(header_len) = write_header(&mut stack, response, len, keep_alive) {
        return sys::write_two(fd, &stack[..header_len], body);
    }
    let mut heap = vec![0u8; response_header_len(response, body.len())];
    let len = write_header(&mut heap, response, body.len(), keep_alive).expect("header buffer");
    heap.truncate(len);
    sys::write_two(fd, &heap, body)
}

fn response_header_len(response: &Response, body_len: usize) -> usize {
    160 + response.content_type.len()
        + digits(body_len)
        + response
            .location
            .as_ref()
            .map(|loc| loc.len() + 12)
            .unwrap_or(0)
}

fn write_header(buf: &mut [u8], response: &Response, len: usize, keep: bool) -> Option<usize> {
    let mut out = Out { buf, n: 0 };
    out.bytes(b"HTTP/1.1 ")?;
    out.status(response.status)?;
    out.byte(b' ')?;
    out.bytes(reason(response.status).as_bytes())?;
    out.bytes(b"\r\nContent-Type: ")?;
    out.bytes(response.content_type.as_bytes())?;
    out.bytes(b"\r\nContent-Length: ")?;
    out.usize(len)?;
    out.bytes(if keep {
        b"\r\nConnection: keep-alive\r\nServer: jango\r\n"
    } else {
        b"\r\nConnection: close\r\nServer: jango\r\n"
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

    fn usize(&mut self, mut value: usize) -> Option<()> {
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

fn digits(mut value: usize) -> usize {
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
mod tests {
    use super::*;
    use crate::routes;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::process;

    fn home(req: &Request) -> Response {
        Response::html(format!("<p>{}</p>", req.path()))
    }

    fn item(req: &Request) -> Response {
        Response::text(req.param("id").unwrap_or(""))
    }

    fn echo(req: &Request) -> Response {
        Response::text(req.body_str().unwrap_or(""))
    }

    #[test]
    fn serves_keep_alive_routes_and_bodies() {
        let port = 20000 + (process::id() % 20_000) as u16;
        let addr = format!("127.0.0.1:{port}");
        let listen = addr.clone();
        thread::spawn(move || {
            let router = routes! {
                "/" => home,
                "/song/<id>" => item,
                "/echo" => echo,
            };
            run(&listen, router).expect("server");
        });

        let mut client = connect(&addr);
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\nGET /song/7 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();
        let first = client.read_response();
        let second = client.read_response();
        assert_eq!(first.0, 200);
        assert_eq!(first.1, b"<p>/</p>");
        assert_eq!(second.0, 200);
        assert_eq!(second.1, b"7");

        let mut post = connect(&addr);
        post.write_all(b"POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\nConnection: close\r\n\r\nsoup")
            .unwrap();
        assert_eq!(post.read_response().1, b"soup");

        let mut missing = connect(&addr);
        missing.write_all(b"GET /nope HTTP/1.0\r\n\r\n").unwrap();
        assert_eq!(missing.read_response().0, 404);

        let mut closed = connect(&addr);
        closed
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        closed.stream.shutdown(std::net::Shutdown::Write).unwrap();
        assert_eq!(closed.read_response().1, b"<p>/</p>");
    }

    struct Client {
        stream: TcpStream,
        buf: Vec<u8>,
    }

    impl Client {
        fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.stream.write_all(bytes)
        }

        fn read_response(&mut self) -> (u16, Vec<u8>) {
            let mut tmp = [0u8; 1024];
            loop {
                if let Some((status, body, used)) = take_response(&self.buf) {
                    self.buf.drain(..used);
                    return (status, body);
                }
                let n = self.stream.read(&mut tmp).expect("read");
                assert!(n > 0, "closed before a full response");
                self.buf.extend_from_slice(&tmp[..n]);
            }
        }
    }

    fn connect(addr: &str) -> Client {
        let mut last = None;
        for _ in 0..50 {
            match TcpStream::connect(addr) {
                Ok(stream) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    return Client {
                        stream,
                        buf: Vec::new(),
                    };
                }
                Err(err) => {
                    last = Some(err);
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }
        panic!("connect {addr}: {}", last.unwrap());
    }

    fn take_response(buf: &[u8]) -> Option<(u16, Vec<u8>, usize)> {
        let split = find_header_end(buf)?;
        let head = std::str::from_utf8(&buf[..split]).unwrap();
        let status: u16 = head[9..12].parse().unwrap();
        let len = head
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        let used = split + len;
        if buf.len() < used {
            return None;
        }
        Some((status, buf[split..used].to_vec(), used))
    }

    fn find_header_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4)
            .position(|mark| mark == b"\r\n\r\n")
            .map(|index| index + 4)
    }
}
