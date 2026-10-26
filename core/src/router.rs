//! URL routing.
//!
//! Static paths are one open-addressed lookup.
//! Patterns such as `/song/<id>` are a short list,
//! checked only when the static map misses.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use crate::http::{Request, Response};
use crate::staticfiles::StaticFiles;

/// A view: one function from a request to a response.
pub type View = for<'a> fn(&'a Request<'a>) -> Response;

// -----------------------------------------------
// Router
// Register routes at startup. The server calls
// `call` once per parsed request.
// -----------------------------------------------

/// Map of paths to views.
#[must_use]
pub struct Router {
    map: StaticMap,
    patterns: Vec<Pattern>,
    missing: View,
    templates: Option<fn(&str) -> Option<&'static str>>,
    files: Option<StaticFiles>,
}

impl Router {
    /// An empty router. Unknown paths return 404.
    pub fn new() -> Self {
        Self {
            map: StaticMap::default(),
            patterns: Vec::new(),
            missing: default_not_found,
            templates: None,
            files: None,
        }
    }

    /// Use these embedded templates when a view calls `render!`.
    pub fn templates(mut self, load: fn(&str) -> Option<&'static str>) -> Self {
        self.templates = Some(load);
        self
    }

    /// Serve files from `dir` at `url`.
    ///
    /// `url` looks like `/static/`. `static/css/style.css` is then
    /// available at `/static/css/style.css`. `{% static %}` uses `url`.
    pub fn staticfiles(mut self, url: &'static str, dir: impl Into<PathBuf>) -> Self {
        self.files = Some(StaticFiles::new(url, dir));
        self
    }

    /// Bind `path` to `view`.
    ///
    /// `path` is exact: `/about` does not match `/about/`.
    /// `<name>` captures one segment, as in `/song/<id>`.
    pub fn route(mut self, path: &'static str, view: View) -> Self {
        assert!(path.starts_with('/'), "route `{path}` must start with /");
        if path.contains('<') {
            assert!(
                !self.patterns.iter().any(|pattern| pattern.raw == path),
                "duplicate route `{path}`"
            );
            self.patterns.push(Pattern::parse(path, view));
        } else {
            self.map.insert(path, view);
        }
        self
    }

    /// View used when no route matches. The default is 404.
    pub fn not_found(mut self, view: View) -> Self {
        self.missing = view;
        self
    }

    /// Static files first, then an exact path, then `<name>` patterns.
    pub(crate) fn call(&self, req: &mut Request<'_>) -> Response {
        req.templates = self.templates;
        req.static_url = self.files.as_ref().map(|files| files.url());
        if let Some(files) = &self.files {
            if let Some(response) = files.serve(req) {
                return response;
            }
        }
        if let Some(view) = self.map.get(req.path) {
            return view(req);
        }
        for pattern in &self.patterns {
            if let Some(count) = pattern.capture(req.path, &mut req.params) {
                req.nparams = count;
                return (pattern.view)(req);
            }
        }
        (self.missing)(req)
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

/// Plain text 404 used until the project sets its own.
fn default_not_found(_req: &Request) -> Response {
    Response::status(404, "Not Found")
}

// -----------------------------------------------
// Static map
// FNV-1a and open addressing. Lookup borrows the
// path and does not allocate.
// -----------------------------------------------

#[derive(Default)]
struct StaticMap {
    slots: Vec<Option<(&'static str, View)>>,
    len: usize,
}

impl StaticMap {
    fn insert(&mut self, key: &'static str, view: View) {
        if self.slots.is_empty() || (self.len + 1) * 2 >= self.slots.len() {
            self.grow();
        }
        self.insert_grown(key, view);
    }

    fn grow(&mut self) {
        let cap = self.slots.len().max(4) * 2;
        let old = std::mem::take(&mut self.slots);
        self.slots = vec![None; cap];
        self.len = 0;
        for (key, view) in old.into_iter().flatten() {
            self.insert_grown(key, view);
        }
    }

    fn insert_grown(&mut self, key: &'static str, view: View) {
        let mask = self.slots.len() - 1;
        let mut index = hash(key) & mask;
        loop {
            match &self.slots[index] {
                None => {
                    self.slots[index] = Some((key, view));
                    self.len += 1;
                    return;
                }
                Some((found, _)) if *found == key => {
                    panic!("duplicate route `{key}`");
                }
                Some(_) => index = (index + 1) & mask,
            }
        }
    }

    fn get(&self, key: &str) -> Option<View> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut index = hash(key) & mask;
        loop {
            match &self.slots[index] {
                None => return None,
                Some((found, view)) if *found == key => return Some(*view),
                Some(_) => index = (index + 1) & mask,
            }
        }
    }
}

/// FNV-1a. The path map uses this so a lookup does not allocate.
fn hash(text: &str) -> usize {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash as usize
}

// -----------------------------------------------
// Patterns
// `/song/<id>` becomes [lit "song", param "id"].
// Matching walks the path segments once.
// -----------------------------------------------

enum Seg {
    Lit(&'static str),
    Param(&'static str),
}

struct Pattern {
    raw: &'static str,
    segs: Vec<Seg>,
    view: View,
}

impl Pattern {
    /// `/song/<id>` becomes a literal segment and a capture named `id`.
    fn parse(path: &'static str, view: View) -> Self {
        let Some(rest) = path.strip_prefix('/') else {
            panic!("route `{path}` must start with /");
        };
        let mut segs = Vec::new();
        for part in rest.split('/') {
            if part.is_empty() {
                panic!("route `{path}` has an empty segment");
            }
            if let Some(name) = part
                .strip_prefix('<')
                .and_then(|text| text.strip_suffix('>'))
            {
                if name.is_empty()
                    || !name
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                {
                    panic!("route `{path}` has an invalid capture name");
                }
                segs.push(Seg::Param(name));
            } else if part.contains('<') || part.contains('>') {
                panic!("route `{path}` has a broken capture");
            } else {
                segs.push(Seg::Lit(part));
            }
        }
        let captures = segs
            .iter()
            .filter(|seg| matches!(seg, Seg::Param(_)))
            .count();
        assert!(captures <= 8, "route `{path}` has more than 8 captures");
        Self {
            raw: path,
            segs,
            view,
        }
    }

    /// Fill `params` when `path` matches. The count is how many captures were stored.
    fn capture<'a>(&self, path: &'a str, params: &mut [(&'a str, &'a str); 8]) -> Option<u8> {
        let mut parts = path.split('/');
        if parts.next() != Some("") {
            return None;
        }
        let mut count = 0u8;
        for seg in &self.segs {
            let part = parts.next()?;
            if part.is_empty() {
                return None;
            }
            match seg {
                Seg::Lit(text) if *text == part => {}
                Seg::Param(name) => {
                    params[count as usize] = (*name, part);
                    count += 1;
                }
                Seg::Lit(_) => return None,
            }
        }
        parts.next().is_none().then_some(count)
    }
}

#[cfg(test)]
#[path = "../tests/router.rs"]
mod tests;
