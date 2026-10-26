//! Static files.
//!
//! `static/css/style.css` is served at `/static/css/style.css`.
//! Pictures, video, and other files stay in the folder.
//! They are not copied into the binary.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::io;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::http::{FileBody, Method, Request, Response};
use crate::sys;

// -----------------------------------------------
// Static files
// One URL prefix and one directory. The directory
// is the project folder while developing, or the
// folder you place next to the program you ship.
// -----------------------------------------------

pub(crate) struct StaticFiles {
    url: &'static str,
    dir: PathBuf,
    /// The folder, opened once. Later requests do not walk the path again.
    root: OnceLock<OwnedFd>,
}

impl StaticFiles {
    /// `url` looks like `/static/`. `dir` is the folder on disk.
    pub(crate) fn new(url: &'static str, dir: impl Into<PathBuf>) -> Self {
        assert!(valid_url(url), "static url `{url}` must look like /static/");
        Self {
            url,
            dir: locate(dir.into()),
            root: OnceLock::new(),
        }
    }

    /// The public prefix, used by `{% static %}`.
    pub(crate) fn url(&self) -> &'static str {
        self.url
    }

    /// `Some` when this path belongs to the static prefix.
    /// A missing file is a 404, not a fall-through to views.
    pub(crate) fn serve(&self, req: &Request<'_>) -> Option<Response> {
        let rel = self.relative(req.path())?;
        Some(self.respond(rel, req))
    }

    /// The path under the prefix. `/static` and `/static/` have no file.
    fn relative<'a>(&self, path: &'a str) -> Option<&'a str> {
        if let Some(rest) = path.strip_prefix(self.url) {
            return Some(rest);
        }
        (path == self.url.trim_end_matches('/')).then_some("")
    }

    /// GET and HEAD serve the file. Any other method is 405.
    ///
    /// A normal file is opened in place, without following a link.
    /// A symlink is checked with the real path and refused if it leaves the folder.
    fn respond(&self, rel: &str, req: &Request<'_>) -> Response {
        if req.method() != Method::Get && req.method() != Method::Head {
            return Response::status(405, "Method Not Allowed");
        }
        let Some(rel) = decode_rel(rel) else {
            return not_found();
        };
        let Some(root) = self.root_fd() else {
            return not_found();
        };
        match open_tree(root, &rel) {
            Ok(file) => file_body(req, &self.dir.join(&rel), Some(file)),
            Err(err) if sys::followed_link(&err) => self.by_real_path(req, &rel),
            Err(_) => not_found(),
        }
    }

    /// The static folder, opened on the first file request.
    fn root_fd(&self) -> Option<&OwnedFd> {
        if let Some(fd) = self.root.get() {
            return Some(fd);
        }
        let file = File::open(&self.dir).ok()?;
        if !file.metadata().ok()?.is_dir() {
            return None;
        }
        let _ = self.root.set(OwnedFd::from(file));
        self.root.get()
    }

    /// Resolve a symlink and serve it only when the real file stays in the folder.
    fn by_real_path(&self, req: &Request<'_>, rel: &Path) -> Response {
        let Ok(root) = self.dir.canonicalize() else {
            return not_found();
        };
        let Ok(path) = root.join(rel).canonicalize() else {
            return not_found();
        };
        if !path.starts_with(&root) {
            return not_found();
        }
        file_body(req, &path, None)
    }
}

/// Open each path piece from the static folder. A symlink stops the walk.
fn open_tree(root: &OwnedFd, rel: &Path) -> io::Result<File> {
    let mut names = Vec::new();
    for piece in rel.components() {
        let std::path::Component::Normal(name) = piece else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "static path"));
        };
        let Some(name) = name.to_str() else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "static path"));
        };
        names.push(name);
    }
    let Some((last, dirs)) = names.split_last() else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "static path"));
    };
    let mut current = None;
    for name in dirs {
        let parent = current.as_ref().unwrap_or(root);
        current = Some(sys::open_at(parent, name, true)?);
    }
    let parent = current.as_ref().unwrap_or(root);
    Ok(File::from(sys::open_at(parent, last, false)?))
}

/// Fill status, length, and range from a file that is already known to be inside the folder.
fn file_body(req: &Request<'_>, path: &Path, opened: Option<File>) -> Response {
    let meta = match &opened {
        Some(file) => file.metadata(),
        None => fs::metadata(path),
    };
    let Ok(meta) = meta else {
        return not_found();
    };
    if !meta.is_file() {
        return not_found();
    }
    let total = meta.len();
    let span = match req.header("range") {
        Some(header) => byte_range(header, total),
        None => Span::All,
    };
    match span {
        Span::All => file_response(200, path, 0, total, None, opened),
        Span::Part { start, len } => {
            let end = start + len - 1;
            file_response(
                206,
                path,
                start,
                len,
                Some(format!("bytes {start}-{end}/{total}")),
                opened,
            )
        }
        Span::Unsatisfiable => unsatisfiable(total),
    }
}

/// A short 404. It does not call the project's `not_found` view.
fn not_found() -> Response {
    Response::status(404, "Not Found")
}

/// A response the server streams from disk, from `start` for `len` bytes.
fn file_response(
    status: u16,
    path: &Path,
    start: u64,
    len: u64,
    content_range: Option<String>,
    opened: Option<File>,
) -> Response {
    let mut res = Response::status(status, "");
    res.content_type = content_type(path);
    res.file = Some(FileBody {
        path: path.to_path_buf(),
        start,
        len,
        opened,
    });
    res.accept_ranges = true;
    res.content_range = content_range;
    res
}

/// 416. The range asked for bytes the file does not have.
fn unsatisfiable(total: u64) -> Response {
    let mut res = Response::status(416, "Range Not Satisfiable");
    res.accept_ranges = true;
    res.content_range = Some(format!("bytes */{total}"));
    res
}

/// `/static/` is valid. `/`, `static`, and a url with `..` are not.
fn valid_url(url: &str) -> bool {
    url.len() > 2
        && url.starts_with('/')
        && url.ends_with('/')
        && !url.contains("//")
        && url.split('/').all(|seg| seg != "." && seg != "..")
        && url
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'?' | b'#' | b'\\' | b'%'))
}

/// An absolute path stays put. A relative one is the current
/// directory, or the folder beside the program.
fn locate(dir: PathBuf) -> PathBuf {
    if dir.is_absolute() {
        return dir;
    }
    if let Ok(cwd) = env::current_dir() {
        let at_cwd = cwd.join(&dir);
        if at_cwd.is_dir() {
            return at_cwd;
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(folder) = exe.parent() {
            let beside = folder.join(&dir);
            if beside.is_dir() {
                return beside;
            }
        }
    }
    env::current_dir().map(|cwd| cwd.join(&dir)).unwrap_or(dir)
}

// -----------------------------------------------
// Paths
// Percent-decoding stays inside the static folder.
// `..`, a symlink that leaves the folder, and a
// directory listing are all refused.
// -----------------------------------------------

/// The URL path as a file path. `..`, an empty segment, and `%2f` fail.
fn decode_rel(text: &str) -> Option<PathBuf> {
    if text.is_empty() {
        return None;
    }
    if text
        .bytes()
        .any(|byte| byte == b'%' || byte == b'\\' || byte == 0)
    {
        return decode_encoded(text);
    }
    let mut path = PathBuf::new();
    for seg in text.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return None;
        }
        path.push(seg);
    }
    Some(path)
}

/// Percent-decode, then apply the same segment rules as `decode_rel`.
fn decode_encoded(text: &str) -> Option<PathBuf> {
    let decoded = percent_decode(text)?;
    let mut path = PathBuf::new();
    for seg in decoded.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return None;
        }
        path.push(seg);
    }
    Some(path)
}

/// Decode `%HH`. A decoded slash, backslash, or NUL is refused.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let byte = (hex(bytes[i + 1])? << 4) | hex(bytes[i + 2])?;
            if byte == 0 || byte == b'/' || byte == b'\\' {
                return None;
            }
            out.push(byte);
            i += 3;
        } else if bytes[i] == b'\\' || bytes[i] == 0 {
            return None;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// One hex digit, or `None` when the byte is not hex.
fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

// -----------------------------------------------
// Ranges
// One `Range: bytes=` span, so a video can seek.
// An unreadable range is 416. Several ranges are
// ignored and the whole file is sent.
// -----------------------------------------------

enum Span {
    All,
    Part { start: u64, len: u64 },
    Unsatisfiable,
}

/// One `bytes=` range. Several ranges are ignored and the whole file is sent.
fn byte_range(header: &str, total: u64) -> Span {
    let Some(spec) = header.trim().strip_prefix("bytes=") else {
        return Span::All;
    };
    if spec.is_empty() || spec.contains(',') {
        return Span::All;
    }
    let Some((start_text, end_text)) = spec.split_once('-') else {
        return Span::All;
    };
    if total == 0 {
        return Span::Unsatisfiable;
    }
    if start_text.is_empty() {
        let Ok(n) = end_text.parse::<u64>() else {
            return Span::All;
        };
        if n == 0 {
            return Span::Unsatisfiable;
        }
        let len = n.min(total);
        return Span::Part {
            start: total - len,
            len,
        };
    }
    let Ok(start) = start_text.parse::<u64>() else {
        return Span::All;
    };
    if start >= total {
        return Span::Unsatisfiable;
    }
    let end = if end_text.is_empty() {
        total - 1
    } else {
        let Ok(end) = end_text.parse::<u64>() else {
            return Span::All;
        };
        if end < start {
            return Span::Unsatisfiable;
        }
        end.min(total - 1)
    };
    Span::Part {
        start,
        len: end - start + 1,
    }
}

// -----------------------------------------------
// Types
// The extension picks the content type. An unknown
// file is still served, as raw bytes.
// -----------------------------------------------

/// The type from the extension. An unknown file is raw bytes.
fn content_type(path: &Path) -> &'static str {
    let mut buf = [0u8; 8];
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return "application/octet-stream";
    };
    if ext.len() > buf.len() || !ext.is_ascii() {
        return "application/octet-stream";
    }
    for (slot, byte) in buf.iter_mut().zip(ext.bytes()) {
        *slot = byte.to_ascii_lowercase();
    }
    match &buf[..ext.len()] {
        b"css" => "text/css; charset=utf-8",
        b"js" | b"mjs" => "text/javascript; charset=utf-8",
        b"html" | b"htm" => "text/html; charset=utf-8",
        b"txt" => "text/plain; charset=utf-8",
        b"csv" => "text/csv; charset=utf-8",
        b"json" => "application/json",
        b"xml" => "application/xml",
        b"pdf" => "application/pdf",
        b"zip" => "application/zip",
        b"wasm" => "application/wasm",
        b"svg" => "image/svg+xml",
        b"png" => "image/png",
        b"jpg" | b"jpeg" => "image/jpeg",
        b"gif" => "image/gif",
        b"webp" => "image/webp",
        b"avif" => "image/avif",
        b"ico" => "image/x-icon",
        b"bmp" => "image/bmp",
        b"mp4" | b"m4v" => "video/mp4",
        b"webm" => "video/webm",
        b"ogv" => "video/ogg",
        b"mov" => "video/quicktime",
        b"mp3" => "audio/mpeg",
        b"wav" => "audio/wav",
        b"ogg" => "audio/ogg",
        b"m4a" => "audio/mp4",
        b"woff" => "font/woff",
        b"woff2" => "font/woff2",
        b"ttf" => "font/ttf",
        b"otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
#[path = "../tests/static.rs"]
mod tests;
