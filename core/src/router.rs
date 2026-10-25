//! URL routing.
//!
//! Static paths are one open-addressed lookup.
//! Patterns such as `/song/<id>` are a short list,
//! checked only when the static map misses.

#![forbid(unsafe_code)]

use crate::http::{Request, Response};

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
}

impl Router {
    pub fn new() -> Self {
        Self {
            map: StaticMap::default(),
            patterns: Vec::new(),
            missing: default_not_found,
        }
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

    pub(crate) fn call(&self, req: &mut Request<'_>) -> Response {
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
mod tests {
    use super::*;
    use crate::http::Method;

    fn home(req: &Request) -> Response {
        Response::text(req.path())
    }

    fn item(req: &Request) -> Response {
        Response::text(req.param("id").unwrap_or(""))
    }

    #[test]
    fn static_and_captured_routes() {
        let router = Router::new().route("/", home).route("/song/<id>", item);
        let headers = [];
        let mut root = Request::new(Method::Get, "/", "", &headers, b"");
        assert_eq!(router.call(&mut root).body, b"/");

        let mut song = Request::new(Method::Get, "/song/12", "", &headers, b"");
        assert_eq!(router.call(&mut song).body, b"12");

        let mut missing = Request::new(Method::Get, "/nope", "", &headers, b"");
        assert_eq!(router.call(&mut missing).status, 404);
    }
}
