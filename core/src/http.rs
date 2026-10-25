//! Request and response types.

#![forbid(unsafe_code)]

use std::fmt;

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
    pub(crate) params: [(&'a str, &'a str); 8],
    pub(crate) nparams: u8,
    pub(crate) templates: Option<fn(&str) -> Option<&'static str>>,
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
        }
    }

    pub fn method(&self) -> Method {
        self.method
    }

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

/// HTTP response returned by a view.
#[derive(Clone, Debug)]
#[must_use]
pub struct Response {
    pub(crate) status: u16,
    pub(crate) content_type: &'static str,
    pub(crate) body: Vec<u8>,
    pub(crate) location: Option<String>,
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
        }
    }

    pub fn text(body: impl Into<String>) -> Self {
        Self::make(200, "text/plain; charset=utf-8", body)
    }

    pub fn html(body: impl Into<String>) -> Self {
        Self::make(200, "text/html; charset=utf-8", body)
    }

    pub fn json(body: impl Into<String>) -> Self {
        Self::make(200, "application/json; charset=utf-8", body)
    }

    pub fn status(status: u16, body: impl Into<String>) -> Self {
        Self::make(status, "text/plain; charset=utf-8", body)
    }

    pub fn redirect(location: impl Into<String>) -> Self {
        let mut res = Self::make(302, "text/plain; charset=utf-8", "");
        res.location = Some(location.into());
        res
    }
}

pub(crate) fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "Response",
    }
}

pub(crate) fn eq_ignore(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_and_header_lookup() {
        let headers = [("Host", "localhost"), ("Accept", "text/plain")];
        let req = Request::new(Method::Get, "/menu", "q=soup&page=2", &headers, b"");
        assert_eq!(req.query("q"), Some("soup"));
        assert_eq!(req.query("missing"), None);
        assert_eq!(req.header("accept"), Some("text/plain"));
        assert_eq!(req.method(), Method::Get);
    }
}
