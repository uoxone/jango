//! Tests for the HTTP/1.1 parser.
//!
//! A full request, a buffer that is still short, and requests that must fail.

use super::*;

fn expect(raw: &[u8]) -> Parsed<'_> {
    match parse(raw) {
        Parse::Hit(hit) => hit,
        Parse::Need => panic!("need more bytes: {}", String::from_utf8_lossy(raw)),
        Parse::Fail(code) => panic!("fail {code}: {}", String::from_utf8_lossy(raw)),
    }
}

#[test]
fn parses_two_pipelined_requests() {
    let raw = b"GET /menu?q=soup HTTP/1.1\r\nHost: localhost\r\n\r\nGET /next HTTP/1.1\r\n\r\n";
    let hit = expect(raw);
    assert_eq!(hit.method, Method::Get);
    assert_eq!(hit.path, "/menu");
    assert_eq!(hit.query, "q=soup");
    assert!(hit.keep_alive);
    assert_eq!(hit.body, b"");
    assert_eq!(hit.header_count, 1);
    assert_eq!(hit.headers[0], ("Host", "localhost"));

    let again = expect(&raw[hit.total..]);
    assert_eq!(again.path, "/next");
    assert_eq!(hit.total + again.total, raw.len());
}

#[test]
fn methods_query_and_body() {
    for (name, method) in [
        ("PUT", Method::Put),
        ("DELETE", Method::Delete),
        ("HEAD", Method::Head),
        ("PATCH", Method::Patch),
        ("OPTIONS", Method::Options),
    ] {
        let raw = format!("{name} / HTTP/1.1\r\n\r\n");
        assert_eq!(expect(raw.as_bytes()).method, method);
    }

    let raw = b"POST /menu?q=soup HTTP/1.1\r\nHost:  localhost\t\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhelloNEXT";
    let hit = expect(raw);
    assert_eq!(hit.path, "/menu");
    assert_eq!(hit.query, "q=soup");
    assert_eq!(hit.body, b"hello");
    assert!(!hit.keep_alive);
    assert_eq!(hit.headers[0], ("Host", "localhost"));
    assert_eq!(&raw[hit.total..], b"NEXT");

    let empty = b"POST / HTTP/1.1\r\nContent-Length: 0\r\n\r\nREST";
    let hit = expect(empty);
    assert_eq!(hit.body, b"");
    assert_eq!(&empty[hit.total..], b"REST");

    let bare = expect(b"GET /? HTTP/1.1\r\n\r\nTAIL");
    assert_eq!(bare.path, "/");
    assert_eq!(bare.query, "");
    assert_eq!(&b"GET /? HTTP/1.1\r\n\r\nTAIL"[bare.total..], b"TAIL");
}

#[test]
fn keep_alive_follows_version_and_connection() {
    assert!(expect(b"GET / HTTP/1.1\r\n\r\n").keep_alive);
    assert!(!expect(b"GET / HTTP/1.0\r\n\r\n").keep_alive);
    assert!(expect(b"GET / HTTP/1.0\r\nConnection: Keep-Alive\r\n\r\n").keep_alive);
    assert!(!expect(b"GET / HTTP/1.1\r\nConnection: Close\r\n\r\n").keep_alive);

    let last = b"GET / HTTP/1.1\r\nConnection: close\r\nConnection: keep-alive\r\n\r\n";
    assert!(expect(last).keep_alive);
}

#[test]
fn waits_until_the_request_is_complete() {
    assert!(matches!(
        parse(b"GET / HTTP/1.1\r\nHost: x\r\n"),
        Parse::Need
    ));
    assert!(matches!(
        parse(b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhel"),
        Parse::Need
    ));
    let exact = format!("POST / HTTP/1.1\r\nContent-Length: {MAX_BODY}\r\n\r\n");
    assert!(matches!(parse(exact.as_bytes()), Parse::Need));
    assert!(matches!(parse(&vec![b'A'; MAX_HEADER]), Parse::Need));

    let full = b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
    assert_eq!(expect(full).body, b"hello");
}

#[test]
fn accepts_32_headers() {
    let mut raw = String::from("GET / HTTP/1.1\r\n");
    for i in 0..32 {
        raw.push_str(&format!("X-{i}: v\r\n"));
    }
    raw.push_str("\r\n");
    assert_eq!(expect(raw.as_bytes()).header_count, 32);
}

#[test]
fn rejects_bad_requests() {
    let cases: &[&[u8]] = &[
        b"GET\r\n\r\n",
        b"GET / HTTP/1.2\r\n\r\n",
        b"get / HTTP/1.1\r\n\r\n",
        b"CONNECT / HTTP/1.1\r\n\r\n",
        b"GET / HTTP/1.1 extra\r\n\r\n",
        b"GET  HTTP/1.1\r\n\r\n",
        b"GET * HTTP/1.1\r\n\r\n",
        b"GET http://h/ HTTP/1.1\r\n\r\n",
        b"GET / HTTP/1.1\r\nBad Name: x\r\n\r\n",
        b"GET / HTTP/1.1\r\n: x\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost\r\n\r\n",
        b"POST / HTTP/1.1\r\nContent-Length: no\r\n\r\n",
        b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
        "GET / HTTP/1.1\r\nHé: v\r\n\r\n".as_bytes(),
    ];
    for raw in cases {
        assert!(
            matches!(parse(raw), Parse::Fail(400)),
            "{}",
            String::from_utf8_lossy(raw)
        );
    }

    let over = format!(
        "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        MAX_BODY + 1
    );
    assert!(matches!(parse(over.as_bytes()), Parse::Fail(413)));

    let mut raw = String::from("GET / HTTP/1.1\r\n");
    for i in 0..33 {
        raw.push_str(&format!("X-{i}: v\r\n"));
    }
    raw.push_str("\r\n");
    assert!(matches!(parse(raw.as_bytes()), Parse::Fail(400)));

    assert!(matches!(
        parse(&vec![b'A'; MAX_HEADER + 1]),
        Parse::Fail(400)
    ));

    let mut utf8 = b"GET / HTTP/1.1\r\nX: ".to_vec();
    utf8.push(0xff);
    utf8.extend_from_slice(b"\r\n\r\n");
    assert!(matches!(parse(&utf8), Parse::Fail(400)));
}
