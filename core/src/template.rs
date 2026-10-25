//! Django-style templates.
//!
//! `{{ name }}` is escaped. `{% extends %}` and `{% block %}`
//! share one page shell. The files live in `templates/`.

#![forbid(unsafe_code)]

use crate::http::{Request, Response};

// -----------------------------------------------
// render
// A view asks for a template by name. The loader
// is the project's embedded `templates/` directory.
// -----------------------------------------------

pub fn render(req: &Request, name: &str, context: &[(&str, &str)]) -> Response {
    let Some(load) = req.templates else {
        return Response::status(500, "templates are not loaded");
    };
    match draw(name, load, req, context) {
        Ok(html) => Response::html(html),
        Err(err) => Response::status(500, err),
    }
}

/// Fill a template from `templates/`.
///
/// `{{ name }}` and `{{ request.path }}` are HTML-escaped.
/// `{% extends "page.html" %}` and `{% block content %}` work
/// the same way as in Django.
#[macro_export]
macro_rules! render {
    ($req:expr, $name:expr) => {
        $crate::render!($req, $name, {})
    };
    ($req:expr, $name:expr, { $($key:literal => $value:expr),* $(,)? }) => {
        $crate::__render($req, $name, &[$(($key, $value)),*] as &[(&str, &str)])
    };
}

// -----------------------------------------------
// Nodes
// One walk of the file. Text is copied through.
// Variables and blocks are the only structure.
// -----------------------------------------------

struct Parsed<'a> {
    parent: Option<&'a str>,
    nodes: Vec<Node<'a>>,
}

enum Node<'a> {
    Text(&'a str),
    Var(&'a str),
    Block(&'a str, Vec<Node<'a>>),
}

struct Cur<'a> {
    src: &'a str,
    i: usize,
}

impl<'a> Cur<'a> {
    fn rest(&self) -> &'a str {
        &self.src[self.i..]
    }

    fn bump(&mut self, n: usize) {
        self.i += n;
    }
}

fn parse(source: &str) -> Result<Parsed<'_>, String> {
    let mut cur = Cur { src: source, i: 0 };
    let parent = leading_extends(&mut cur)?;
    let nodes = parse_nodes(&mut cur, None)?;
    Ok(Parsed { parent, nodes })
}

fn leading_extends<'a>(cur: &mut Cur<'a>) -> Result<Option<&'a str>, String> {
    let start = cur.i;
    loop {
        let rest = cur.rest();
        if rest.starts_with("{#") {
            cur.bump(2);
            let _ = take_until(cur, "#}")?;
            continue;
        }
        let trimmed = rest.trim_start_matches(|ch: char| ch.is_ascii_whitespace());
        if trimmed.len() != rest.len() {
            cur.bump(rest.len() - trimmed.len());
            continue;
        }
        break;
    }
    if cur.rest().starts_with("{%") {
        let mut look = Cur {
            src: cur.src,
            i: cur.i,
        };
        look.bump(2);
        let body = take_until(&mut look, "%}")?;
        if let Tag::Extends(name) = parse_tag(body)? {
            cur.i = look.i;
            return Ok(Some(name));
        }
    }
    cur.i = start;
    Ok(None)
}

fn parse_nodes<'a>(cur: &mut Cur<'a>, stop: Option<&str>) -> Result<Vec<Node<'a>>, String> {
    let mut nodes = Vec::new();
    loop {
        if cur.i == cur.src.len() {
            if let Some(name) = stop {
                return Err(format!("unclosed block `{name}`"));
            }
            return Ok(nodes);
        }
        let rest = cur.rest();
        if rest.starts_with("{#") {
            cur.bump(2);
            let _ = take_until(cur, "#}")?;
            continue;
        }
        if rest.starts_with("{{") {
            cur.bump(2);
            let body = take_until(cur, "}}")?;
            nodes.push(Node::Var(parse_var(body)?));
            continue;
        }
        if rest.starts_with("{%") {
            cur.bump(2);
            let body = take_until(cur, "%}")?;
            match parse_tag(body)? {
                Tag::EndBlock(name) => {
                    let Some(open) = stop else {
                        return Err("unexpected endblock".into());
                    };
                    if name.is_some_and(|name| name != open) {
                        return Err(format!("endblock does not match `{open}`"));
                    }
                    return Ok(nodes);
                }
                Tag::Block(name) => {
                    let body = parse_nodes(cur, Some(name))?;
                    nodes.push(Node::Block(name, body));
                }
                Tag::Extends(_) => return Err("extends must be the first tag".into()),
            }
            continue;
        }
        let end = next_tag(rest).unwrap_or(rest.len());
        if end == 0 {
            return Err("broken template tag".into());
        }
        nodes.push(Node::Text(&rest[..end]));
        cur.bump(end);
    }
}

fn next_tag(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && matches!(bytes[i + 1], b'{' | b'%' | b'#') {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn take_until<'a>(cur: &mut Cur<'a>, closer: &str) -> Result<&'a str, String> {
    let rest = cur.rest();
    let Some(at) = rest.find(closer) else {
        return Err("unclosed template tag".into());
    };
    let body = &rest[..at];
    cur.bump(at + closer.len());
    Ok(body)
}

enum Tag<'a> {
    Extends(&'a str),
    Block(&'a str),
    EndBlock(Option<&'a str>),
}

fn parse_tag(body: &str) -> Result<Tag<'_>, String> {
    let body = body.trim();
    let (word, rest) = split_word(body);
    let rest = rest.trim();
    match word {
        "extends" => Ok(Tag::Extends(quoted(rest)?)),
        "block" => {
            if !ident(rest) {
                return Err(format!("invalid block `{rest}`"));
            }
            Ok(Tag::Block(rest))
        }
        "endblock" => {
            if rest.is_empty() {
                return Ok(Tag::EndBlock(None));
            }
            if !ident(rest) {
                return Err(format!("invalid endblock `{rest}`"));
            }
            Ok(Tag::EndBlock(Some(rest)))
        }
        other => Err(format!("unknown tag `{other}`")),
    }
}

fn parse_var(body: &str) -> Result<&str, String> {
    let name = body.trim();
    if !valid_var(name) {
        return Err(format!("invalid variable `{name}`"));
    }
    Ok(name)
}

fn valid_var(name: &str) -> bool {
    let mut parts = name.split('.');
    let Some(root) = parts.next() else {
        return false;
    };
    if !ident(root) {
        return false;
    }
    match parts.next() {
        None => true,
        Some(field) => ident(field) && parts.next().is_none(),
    }
}

fn ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn split_word(text: &str) -> (&str, &str) {
    text.split_once(|ch: char| ch.is_ascii_whitespace())
        .unwrap_or((text, ""))
}

fn quoted(text: &str) -> Result<&str, String> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 {
        return Err("template name must be quoted".into());
    }
    let quote = bytes[0];
    if (quote != b'"' && quote != b'\'') || bytes[bytes.len() - 1] != quote {
        return Err("template name must be quoted".into());
    }
    let name = &text[1..text.len() - 1];
    if name.is_empty() || name.bytes().any(|b| b == b'\\' || b == b'\n' || b == b'\r') {
        return Err("invalid template name".into());
    }
    Ok(name)
}

// -----------------------------------------------
// Inheritance
// The child replaces blocks in the parent.
// The deepest child wins. A cycle is an error.
// -----------------------------------------------

fn draw(
    name: &str,
    load: fn(&str) -> Option<&'static str>,
    req: &Request,
    context: &[(&str, &str)],
) -> Result<String, String> {
    let mut names = Vec::new();
    let mut chain = Vec::new();
    let mut current = name;
    loop {
        if names.iter().any(|seen| *seen == current) {
            return Err(format!("template `{current}` extends itself"));
        }
        if names.len() == 16 {
            return Err("template extends too deep".into());
        }
        names.push(current);
        let source = load(current).ok_or_else(|| format!("template `{current}` was not found"))?;
        let parsed = parse(source)?;
        let parent = parsed.parent;
        chain.push(parsed);
        match parent {
            Some(next) => current = next,
            None => break,
        }
    }

    let base = chain.len() - 1;
    let mut blocks: Vec<(&str, Vec<Node<'_>>)> = Vec::new();
    for index in (0..base).rev() {
        for node in std::mem::take(&mut chain[index].nodes) {
            let Node::Block(name, body) = node else {
                continue;
            };
            if let Some(slot) = blocks.iter_mut().find(|(found, _)| *found == name) {
                slot.1 = body;
            } else {
                blocks.push((name, body));
            }
        }
    }

    let chosen: Vec<(&str, &[Node<'_>])> = blocks
        .iter()
        .map(|(name, body)| (*name, body.as_slice()))
        .collect();
    let mut out = String::new();
    write(&mut out, &chain[base].nodes, &chosen, req, context);
    Ok(out)
}

fn write(
    out: &mut String,
    nodes: &[Node<'_>],
    blocks: &[(&str, &[Node<'_>])],
    req: &Request,
    context: &[(&str, &str)],
) {
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Var(name) => push_escaped(out, lookup(name, req, context)),
            Node::Block(name, body) => {
                let inner = blocks
                    .iter()
                    .find(|(found, _)| found == name)
                    .map(|(_, body)| *body)
                    .unwrap_or(body);
                write(out, inner, &[], req, context);
            }
        }
    }
}

fn lookup<'a>(name: &str, req: &'a Request<'a>, context: &'a [(&str, &str)]) -> &'a str {
    if let Some(field) = name.strip_prefix("request.") {
        return match field {
            "path" => req.path(),
            "method" => req.method().as_str(),
            _ => "",
        };
    }
    context
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
        .unwrap_or("")
}

fn push_escaped(out: &mut String, text: &str) {
    let mut start = 0;
    for (i, ch) in text.char_indices() {
        let repl = match ch {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' => "&quot;",
            '\'' => "&#x27;",
            _ => continue,
        };
        out.push_str(&text[start..i]);
        out.push_str(repl);
        start = i + ch.len_utf8();
    }
    out.push_str(&text[start..]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;

    fn load(name: &str) -> Option<&'static str> {
        Some(match name {
            "page.html" => {
                "a { color: red }\n<title>{% block title %}{% endblock %}</title>\n{% block content %}{% endblock %}"
            }
            "home.html" => {
                "{% extends \"page.html\" %}\n{% block title %}Home{% endblock %}\n{% block content %}<h1>{{ name }}</h1>{% endblock %}"
            }
            "about.html" => {
                "{% extends \"page.html\" %}{% block content %}{{ request.path }} {{ request.method }}{% endblock %}"
            }
            "plain.html" => "Hello {# secret #}{{ name }}",
            "loop.html" => "{% extends \"loop.html\" %}",
            "bad.html" => "{% if x %}{% endif %}",
            "open.html" => "{{ name",
            _ => return None,
        })
    }

    fn sample(path: &str) -> Request<'_> {
        let mut req = Request::new(Method::Get, path, "", &[], b"");
        req.templates = Some(load);
        req
    }

    fn body(res: &Response) -> String {
        String::from_utf8(res.body.clone()).unwrap()
    }

    #[test]
    fn escapes_values_and_keeps_markup() {
        let req = sample("/");
        let res = render(&req, "home.html", &[("name", "<b>\"&'")]);
        let page = body(&res);
        assert_eq!(res.status, 200);
        assert!(page.contains("a { color: red }"));
        assert!(page.contains("<title>Home</title>"));
        assert!(page.contains("<h1>&lt;b&gt;&quot;&amp;&#x27;</h1>"));
        assert!(!page.contains("<b>"));
    }

    #[test]
    fn request_fields_and_comments() {
        let req = sample("/about");
        let res = render(&req, "about.html", &[]);
        assert_eq!(body(&res), "a { color: red }\n<title></title>\n/about GET");

        let res = render(&req, "plain.html", &[("name", "Ada")]);
        assert_eq!(body(&res), "Hello Ada");
    }

    #[test]
    fn template_errors() {
        let req = sample("/");
        let empty: &[(&str, &str)] = &[];
        for (name, needle) in [
            ("missing.html", "was not found"),
            ("loop.html", "extends itself"),
            ("bad.html", "unknown tag"),
            ("open.html", "unclosed"),
        ] {
            let res = render(&req, name, empty);
            assert_eq!(res.status, 500, "{name}");
            assert!(body(&res).contains(needle), "{name}: {}", body(&res));
        }
    }
}
