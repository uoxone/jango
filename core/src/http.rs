//! Request and response types.

#![forbid(unsafe_code)]

use std::fmt;
use std::path::PathBuf;

// -----------------------------------------------
// Method
// The HTTP verb. Views read it when a path serves
// more than one action.
// -----------------------------------------------

/// HTTP method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Head,
    Patch,
    Options,
}

impl Method {
    /// The method token from the request line, such as `"GET"`.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "GET" => Self::Get,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "DELETE" => Self::Delete,
            "HEAD" => Self::Head,
            "PATCH" => Self::Patch,
            "OPTIONS" => Self::Options,
            _ => return None,
        })
    }

    /// `"GET"`, `"POST"`, and so on.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Patch => "PATCH",
            Self::Options => "OPTIONS",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// -----------------------------------------------
// Request
// Borrowed from the connection buffer. The path,
// query, headers, and body are not copied.
// -----------------------------------------------

/// One HTTP request.
///
/// Fields borrow the connection buffer, so they are
/// only valid inside the view.
pub struct Request<'a> {
    pub(crate) method: Method,
    pub(crate) path: &'a str,
    pub(crate) query: &'a str,
    pub(crate) headers: &'a [(&'a str, &'a str)],
    pub(crate) body: &'a [u8],
    /// Values captured by `<name>` segments. At most eight.
    pub(crate) params: [(&'a str, &'a str); 8],
    pub(crate) nparams: u8,
    /// Embedded templates for `render!`. The router sets this.
    pub(crate) templates: Option<fn(&str) -> Option<&'static str>>,
    /// `STATIC_URL`, such as `/static/`. `{% static %}` uses it.
    pub(crate) static_url: Option<&'static str>,
}

impl<'a> Request<'a> {
    pub(crate) fn new(
        method: Method,
        path: &'a str,
        query: &'a str,
        headers: &'a [(&'a str, &'a str)],
        body: &'a [u8],
    ) -> Self {
        Self {
            method,
            path,
            query,
            headers,
            body,
            params: [("", ""); 8],
            nparams: 0,
            templates: None,
            static_url: None,
        }
    }

    /// `GET`, `POST`, and the other verbs.
    pub fn method(&self) -> Method {
        self.method
    }

    /// The path without the query, such as `/song/12`.
    pub fn path(&self) -> &'a str {
        self.path
    }

    /// Query value for `name`, from `?name=value`.
    pub fn query(&self, name: &str) -> Option<&'a str> {
        self.query.split('&').find_map(|pair| {
            if pair.is_empty() {
                return None;
            }
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (key == name).then_some(value)
        })
    }

    /// Value captured by a `<name>` segment in the route.
    pub fn param(&self, name: &str) -> Option<&'a str> {
        self.params[..self.nparams as usize]
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
    }

    /// Header value. The name is case-insensitive.
    pub fn header(&self, name: &str) -> Option<&'a str> {
        self.headers
            .iter()
            .find(|(key, _)| eq_ignore(key, name))
            .map(|(_, value)| *value)
    }

    /// Raw body bytes. Empty when the request sent no body.
    pub fn body(&self) -> &'a [u8] {
        self.body
    }

    /// Body as text, when it is valid UTF-8.
    pub fn body_str(&self) -> Option<&'a str> {
        std::str::from_utf8(self.body).ok()
    }
}

impl fmt::Debug for Request<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("query", &self.query)
            .finish()
    }
}

// -----------------------------------------------
// Response
// A status, a content type, and a body. Helpers
// cover the common cases so views stay one line.
// -----------------------------------------------

/// A file to stream after the headers.
///
/// `start` is the first byte. `len` is how many follow.
/// `opened` is set when the file was already checked and opened.
#[derive(Debug)]
pub(crate) struct FileBody {
    pub path: PathBuf,
    pub start: u64,
    pub len: u64,
    pub opened: Option<std::fs::File>,
}

impl Clone for FileBody {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            start: self.start,
            len: self.len,
            opened: self.opened.as_ref().and_then(|file| file.try_clone().ok()),
        }
    }
}

/// HTTP response returned by a view.
#[derive(Clone, Debug)]
#[must_use]
pub struct Response {
    pub(crate) status: u16,
    pub(crate) content_type: &'static str,
    pub(crate) body: Vec<u8>,
    /// Set by `redirect`. Sent as the `Location` header.
    pub(crate) location: Option<String>,
    /// Set for a static file. The server streams this instead of `body`.
    pub(crate) file: Option<FileBody>,
    /// `bytes 0-99/400` on a partial response, or `bytes */400` on 416.
    pub(crate) content_range: Option<String>,
    /// Sends `Accept-Ranges: bytes` for static files.
    pub(crate) accept_ranges: bool,
}

impl Response {
    fn make(status: u16, content_type: &'static str, body: impl Into<String>) -> Self {
        assert!(
            (100..600).contains(&status),
            "http status must be between 100 and 599"
        );
        Self {
            status,
            content_type,
            body: body.into().into_bytes(),
            location: None,
            file: None,
            content_range: None,
            accept_ranges: false,
        }
    }

    /// Bytes the client should receive: the file slice, or the memory body.
    pub(crate) fn output_len(&self) -> u64 {
        self.file
            .as_ref()
            .map(|file| file.len)
            .unwrap_or(self.body.len() as u64)
    }

    /// 200, `text/plain; charset=utf-8`.
    pub fn text(body: impl Into<String>) -> Self {
        Self::make(200, "text/plain; charset=utf-8", body)
    }

    /// 200, `text/html; charset=utf-8`.
    pub fn html(body: impl Into<String>) -> Self {
        Self::make(200, "text/html; charset=utf-8", body)
    }

    /// 200, `application/json; charset=utf-8`.
    pub fn json(body: impl Into<String>) -> Self {
        Self::make(200, "application/json; charset=utf-8", body)
    }

    /// Any status from 100 through 599, as plain text.
    pub fn status(status: u16, body: impl Into<String>) -> Self {
        Self::make(status, "text/plain; charset=utf-8", body)
    }

    /// 302 with a `Location` header and an empty body.
    pub fn redirect(location: impl Into<String>) -> Self {
        let mut res = Self::make(302, "text/plain; charset=utf-8", "");
        res.location = Some(location.into());
        res
    }
}

/// Reason phrase for the status line. An unknown code uses `"Response"`.
pub(crate) fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        416 => "Range Not Satisfiable",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "Response",
    }
}

/// ASCII case-insensitive equality. Header names use this.
pub(crate) fn eq_ignore(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(&b))
}

#[cfg(test)]
#[path = "../tests/http.rs"]
mod tests;
