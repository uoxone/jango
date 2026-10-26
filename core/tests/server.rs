//! Tests for response headers and a real server on `127.0.0.1`.

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

fn away(_req: &Request) -> Response {
    Response::redirect("/next")
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
            "/away" => away,
        };
        run(&listen, router).expect("server");
    });

    let mut client = connect(&addr);
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\nGET /song/7?x=1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
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

    let mut head = connect(&addr);
    head.write_all(b"HEAD / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let (headers, rest) = head.read_until_close();
    assert!(headers.contains("HTTP/1.1 200 "));
    assert!(headers.contains("Content-Length: 8"));
    assert!(rest.is_empty(), "head body {rest:?}");

    let mut jump = connect(&addr);
    jump.write_all(b"GET /away HTTP/1.0\r\n\r\n").unwrap();
    let (headers, _) = jump.read_until_close();
    assert!(headers.starts_with("HTTP/1.1 302 "));
    assert!(headers.to_ascii_lowercase().contains("location: /next"));
}

#[test]
fn static_range_header() {
    let mut res = Response::status(206, "");
    res.content_type = "video/mp4";
    res.body.clear();
    res.accept_ranges = true;
    res.content_range = Some("bytes 2-5/10".into());
    res.file = Some(crate::http::FileBody {
        path: std::path::PathBuf::from("clip.mp4"),
        start: 2,
        len: 4,
        opened: None,
    });
    let mut buf = vec![0u8; response_header_len(&res, res.output_len())];
    let n = write_header(&mut buf, &res, res.output_len(), false).unwrap();
    assert_eq!(
        std::str::from_utf8(&buf[..n]).unwrap(),
        "HTTP/1.1 206 Partial Content\r\nContent-Type: video/mp4\r\nContent-Length: 4\r\nAccept-Ranges: bytes\r\nContent-Range: bytes 2-5/10\r\nConnection: close\r\nServer: ongo\r\n\r\n"
    );
}

#[test]
fn serves_a_static_file_over_http() {
    let dir = std::env::temp_dir().join(format!("ongo-http-static-{}", process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("site.css"), b"body{color:red}").unwrap();
    std::fs::write(dir.join("clip.mp4"), b"0123456789").unwrap();
    let port = 50000 + (process::id() % 8000) as u16;
    let addr = format!("127.0.0.1:{port}");
    let listen = addr.clone();
    let root = dir.clone();
    thread::spawn(move || {
        let router = routes! { "/" => home }.staticfiles("/static/", root);
        run(&listen, router).expect("server");
    });

    let mut css = connect(&addr);
    css.write_all(
        b"GET /static/site.css?v=1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .unwrap();
    let (headers, body) = css.read_until_close();
    assert!(headers.contains("HTTP/1.1 200 "), "{headers}");
    assert!(
        headers.contains("Content-Type: text/css; charset=utf-8"),
        "{headers}"
    );
    assert!(headers.contains("Accept-Ranges: bytes"), "{headers}");
    assert!(headers.contains("Content-Length: 15"), "{headers}");
    assert_eq!(body, b"body{color:red}");

    let mut clip = connect(&addr);
    clip.write_all(
        b"GET /static/clip.mp4 HTTP/1.1\r\nHost: localhost\r\nRange: bytes=2-5\r\nConnection: close\r\n\r\n",
    )
    .unwrap();
    let (headers, body) = clip.read_until_close();
    assert!(headers.contains("HTTP/1.1 206 "), "{headers}");
    assert!(headers.contains("Content-Type: video/mp4"), "{headers}");
    assert!(headers.contains("Content-Range: bytes 2-5/10"), "{headers}");
    assert_eq!(body, b"2345");

    let mut head = connect(&addr);
    head.write_all(
        b"HEAD /static/clip.mp4 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .unwrap();
    let (headers, body) = head.read_until_close();
    assert!(headers.contains("Content-Length: 10"), "{headers}");
    assert!(body.is_empty(), "{body:?}");

    let mut page = connect(&addr);
    page.write_all(b"GET / HTTP/1.0\r\n\r\n").unwrap();
    assert_eq!(page.read_response().1, b"<p>/</p>");

    let mut missing = connect(&addr);
    missing
        .write_all(b"GET /static/nope.css HTTP/1.0\r\n\r\n")
        .unwrap();
    assert_eq!(missing.read_response().0, 404);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resolves_ipv4_only() {
    let addr = resolve("127.0.0.1:8000").unwrap();
    assert_eq!(addr.ip().octets(), [127, 0, 0, 1]);
    assert_eq!(addr.port(), 8000);
    let err = resolve("[::1]:8000").unwrap_err();
    assert!(err.to_string().contains("IPv4"));
    assert!(resolve("127.0.0.1:99999").is_err());
    assert!(resolve("nope").is_err());
}

#[test]
fn writes_status_length_and_location() {
    let text = Response::text("hi");
    let mut buf = [0u8; 256];
    let n = write_header(&mut buf, &text, text.body.len() as u64, true).unwrap();
    assert_eq!(
        std::str::from_utf8(&buf[..n]).unwrap(),
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 2\r\nConnection: keep-alive\r\nServer: ongo\r\n\r\n"
    );

    let jump = Response::redirect("/next");
    let n = write_header(&mut buf, &jump, 0, false).unwrap();
    assert_eq!(
        std::str::from_utf8(&buf[..n]).unwrap(),
        "HTTP/1.1 302 Found\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 0\r\nConnection: close\r\nServer: ongo\r\nLocation: /next\r\n\r\n"
    );
    assert!(write_header(&mut [0u8; 8], &text, 2, true).is_none());

    for code in [100u16, 200, 404, 599] {
        let res = Response::status(code, "");
        let n = write_header(&mut buf, &res, 0, false).unwrap();
        let header = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(header.starts_with(&format!("HTTP/1.1 {code} ")), "{header}");
        assert!(header.contains(reason(code)), "{header}");
    }
}

#[test]
fn content_length_width() {
    assert_eq!(digits(0), 1);
    assert_eq!(digits(9), 1);
    assert_eq!(digits(10), 2);
    assert_eq!(digits(999), 3);
    assert_eq!(digits(u64::MAX), 20);

    let res = Response::status(404, "");
    let mut buf = vec![0u8; response_header_len(&res, u64::MAX)];
    let n = write_header(&mut buf, &res, u64::MAX, true).unwrap();
    let header = std::str::from_utf8(&buf[..n]).unwrap();
    assert!(header.contains(&format!("Content-Length: {}\r\n", u64::MAX)));
    assert!(header.contains("404 Not Found"));
}

#[test]
fn connection_slots_are_reused() {
    let addr: std::net::SocketAddrV4 = "127.0.0.1:0".parse().unwrap();
    let mut table = Table::default();
    let first_fd = sys::listen_socket(addr).unwrap();
    let first = table.insert(first_fd);
    table
        .get_mut(first)
        .unwrap()
        .buf
        .extend_from_slice(b"stale");
    let second_fd = sys::listen_socket(addr).unwrap();
    let second = table.insert(second_fd);
    assert_eq!(table.len, 2);
    assert_ne!(first, second);

    drop_conn(&mut table, -1, first);
    assert_eq!(table.len, 1);
    let reused_fd = sys::listen_socket(addr).unwrap();
    let reused = table.insert(reused_fd);
    assert_eq!(reused, first);
    let conn = table.get_mut(reused).unwrap();
    assert_eq!(conn.fd, reused_fd);
    assert!(conn.buf.is_empty());

    drop_conn(&mut table, -1, 40);
    assert_eq!(table.len, 2);
    drop_conn(&mut table, -1, reused);
    drop_conn(&mut table, -1, second);
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

    fn read_until_close(&mut self) -> (String, Vec<u8>) {
        let mut tmp = [0u8; 1024];
        loop {
            match self.stream.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(err)
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(err) => panic!("{err}"),
            }
        }
        let end = find_header_end(&self.buf).expect("headers");
        let headers = String::from_utf8(self.buf[..end].to_vec()).unwrap();
        let rest = self.buf[end..].to_vec();
        (headers, rest)
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
