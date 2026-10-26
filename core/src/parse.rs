//! HTTP/1.1 request parser.
//!
//! The parser never allocates. It returns slices into
//! the bytes the socket already read.

#![forbid(unsafe_code)]

use crate::http::{eq_ignore, Method};

pub(crate) const MAX_HEADER: usize = 16 * 1024;
pub(crate) const MAX_BODY: usize = 2 * 1024 * 1024;

// -----------------------------------------------
// Parse result
// Need means the socket has not delivered the full
// request yet. Fail is a status the server sends
// back, then it closes the connection.
// -----------------------------------------------

// `Hit` holds the header slices on the stack. Boxing them would
// allocate on every request, which costs more than the extra bytes.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Parse<'a> {
    Need,
    Fail(u16),
    Hit(Parsed<'a>),
}

pub(crate) struct Parsed<'a> {
    pub method: Method,
    pub path: &'a str,
    pub query: &'a str,
    pub headers: [(&'a str, &'a str); 32],
    pub header_count: usize,
    pub body: &'a [u8],
    pub total: usize,
    pub keep_alive: bool,
}

/// Parse one request from the front of `buf`.
pub(crate) fn parse(buf: &[u8]) -> Parse<'_> {
    let Some(end) = find_header_end(buf) else {
        return if buf.len() > MAX_HEADER {
            Parse::Fail(400)
        } else {
            Parse::Need
        };
    };

    let Ok(head) = std::str::from_utf8(&buf[..end - 4]) else {
        return Parse::Fail(400);
    };
    let mut lines = head.split("\r\n");
    let Some(request_line) = lines.next() else {
        return Parse::Fail(400);
    };
    let Some((method, target, version)) = split_request_line(request_line) else {
        return Parse::Fail(400);
    };
    let Some(method) = Method::parse(method) else {
        return Parse::Fail(400);
    };
    let http11 = match version {
        "HTTP/1.1" => true,
        "HTTP/1.0" => false,
        _ => return Parse::Fail(400),
    };
    let Some((path, query)) = split_target(target) else {
        return Parse::Fail(400);
    };

    let mut headers = [("", ""); 32];
    let mut header_count = 0;
    let mut content_length = None;
    for line in lines {
        let Some((name, value)) = split_header(line) else {
            return Parse::Fail(400);
        };
        if eq_ignore(name, "content-length") {
            if content_length.is_some() {
                return Parse::Fail(400);
            }
            let Ok(length) = value.parse::<usize>() else {
                return Parse::Fail(400);
            };
            if length > MAX_BODY {
                return Parse::Fail(413);
            }
            content_length = Some(length);
        } else if eq_ignore(name, "transfer-encoding") {
            // Stage 1 speaks Content-Length, not chunked bodies.
            return Parse::Fail(400);
        }
        if header_count == headers.len() {
            return Parse::Fail(400);
        }
        headers[header_count] = (name, value);
        header_count += 1;
    }

    let body_len = content_length.unwrap_or(0);
    let total = end + body_len;
    if buf.len() < total {
        return Parse::Need;
    }

    let headers_ref = &headers[..header_count];
    Parse::Hit(Parsed {
        method,
        path,
        query,
        keep_alive: keep_alive(http11, headers_ref),
        headers,
        header_count,
        body: &buf[end..total],
        total,
    })
}

/// Index just past the headers, or `None` while `\r\n\r\n` has not arrived.
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|mark| mark == b"\r\n\r\n")
        .map(|index| index + 4)
}

/// `GET /path HTTP/1.1` into method, target, and version.
fn split_request_line(line: &str) -> Option<(&str, &str, &str)> {
    let mut parts = line.split(' ');
    let method = parts.next()?;
    let target = parts.next()?;
    let version = parts.next()?;
    if parts.next().is_some() || method.is_empty() {
        return None;
    }
    Some((method, target, version))
}

/// `/song/1?x=1` into the path and the query. The target must start with `/`.
fn split_target(target: &str) -> Option<(&str, &str)> {
    if !target.starts_with('/') {
        return None;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    (!path.is_empty()).then_some((path, query))
}

/// `Name: value`. The name is non-empty ASCII with no spaces.
fn split_header(line: &str) -> Option<(&str, &str)> {
    let (name, value) = line.split_once(':')?;
    if name.is_empty()
        || name
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || !byte.is_ascii())
    {
        return None;
    }
    Some((name, value.trim_matches(|c: char| c == ' ' || c == '\t')))
}

/// HTTP/1.1 stays open. `Connection: close` ends it, and HTTP/1.0 needs `keep-alive`.
fn keep_alive(http11: bool, headers: &[(&str, &str)]) -> bool {
    let mut keep = http11;
    for (name, value) in headers {
        if eq_ignore(name, "connection") {
            if eq_ignore(value, "close") {
                keep = false;
            } else if eq_ignore(value, "keep-alive") {
                keep = true;
            }
        }
    }
    keep
}

#[cfg(test)]
#[path = "../tests/parse.rs"]
mod tests;
