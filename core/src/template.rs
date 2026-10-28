//! Django-style templates.
//!
//! `{{ name }}` is escaped. `{% extends %}` and `{% block %}`
//! share one page shell. `{% include %}` renders another file
//! into the page. `{% static %}` points at a file in `static/`.
//! The HTML lives in `templates/`.

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
/// the same way as in Django. `{% include "nav.html" %}` renders
/// that file with the same values. `{% static "css/style.css" %}`
/// prints the public path of a file in the static folder.
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
// Variables, blocks, and includes are the structure.
// -----------------------------------------------

struct Parsed<'a> {
    parent: Option<&'a str>,
    nodes: Vec<Node<'a>>,
}

enum Node<'a> {
    Text(&'a str),
    Var(&'a str),
    Block(&'a str, Vec<Node<'a>>),
    Static(&'a str),
    Include(&'a str),
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

/// Turn one template file into a parent name and a list of nodes.
fn parse(source: &str) -> Result<Parsed<'_>, String> {
    let mut cur = Cur { src: source, i: 0 };
    let parent = leading_extends(&mut cur)?;
    let nodes = parse_nodes(&mut cur, None)?;
    Ok(Parsed { parent, nodes })
}

/// `{% extends %}` counts only as the first tag. Comments and blank lines may sit above it.
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

/// Read nodes until the file ends, or until `{% endblock %}` when `stop` is set.
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
                Tag::Static(name) => nodes.push(Node::Static(name)),
                Tag::Include(name) => nodes.push(Node::Include(name)),
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

/// Byte index of the next `{{`, `{%`, or `{#`.
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

/// Text up to `closer`, then move past the closer.
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
    Static(&'a str),
    Include(&'a str),
}

/// One tag: `extends`, `block`, `endblock`, `static`, or `include`.
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
        "static" => {
            let name = quoted(rest)?;
            if !static_path(name) {
                return Err("invalid static path".into());
            }
            Ok(Tag::Static(name))
        }
        "include" => Ok(Tag::Include(include_name(rest)?)),
        other => Err(format!("unknown tag `{other}`")),
    }
}

/// `{% include "nav.html" %}`. The name is quoted, and nothing follows it.
fn include_name(text: &str) -> Result<&str, String> {
    let (name, rest) = one_quoted(text)?;
    if !rest.is_empty() {
        return Err("invalid include".into());
    }
    Ok(name)
}

/// A quoted string, then whatever text follows the closing quote.
fn one_quoted(text: &str) -> Result<(&str, &str), String> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 {
        return Err("template name must be quoted".into());
    }
    let quote = bytes[0];
    if quote != b'"' && quote != b'\'' {
        return Err("template name must be quoted".into());
    }
    let body = &text[1..];
    let Some(end) = body.bytes().position(|byte| byte == quote) else {
        return Err("template name must be quoted".into());
    };
    let name = &body[..end];
    if name.is_empty()
        || name
            .bytes()
            .any(|byte| byte == b'\\' || byte == b'\n' || byte == b'\r')
    {
        return Err("invalid template name".into());
    }
    Ok((name, body[end + 1..].trim_start()))
}

/// A variable such as `name` or `request.path`.
fn parse_var(body: &str) -> Result<&str, String> {
    let name = body.trim();
    if !valid_var(name) {
        return Err(format!("invalid variable `{name}`"));
    }
    Ok(name)
}

/// `name` or `name.field`. Both parts are identifiers.
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

/// A relative static path. No leading slash and no `..`.
fn static_path(name: &str) -> bool {
    !name.starts_with('/')
        && !name.ends_with('/')
        && name.split('/').all(|seg| {
            !seg.is_empty()
                && seg != "."
                && seg != ".."
                && !seg.contains('\\')
                && !seg.contains('\0')
        })
}

/// A letter or `_`, then letters, digits, or `_`.
fn ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// The first word, and everything after it.
fn split_word(text: &str) -> (&str, &str) {
    text.split_once(|ch: char| ch.is_ascii_whitespace())
        .unwrap_or((text, ""))
}

/// A single- or double-quoted name, with no escapes inside.
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
// {% include %} renders another file and pastes the HTML.
// -----------------------------------------------

/// Load the extends chain, apply the child's blocks, and return HTML.
fn draw(
    name: &str,
    load: fn(&str) -> Option<&'static str>,
    req: &Request,
    context: &[(&str, &str)],
) -> Result<String, String> {
    let mut stack = Vec::new();
    draw_at(name, load, req, context, &mut stack)
}

/// Render one template. A nested `{% include %}` calls this again.
fn draw_at<'a>(
    name: &'a str,
    load: fn(&str) -> Option<&'static str>,
    req: &Request<'_>,
    context: &[(&str, &str)],
    stack: &mut Vec<&'a str>,
) -> Result<String, String> {
    if stack.iter().any(|seen| *seen == name) {
        return Err(format!("template `{name}` includes itself"));
    }
    if stack.len() == 16 {
        return Err("template include too deep".into());
    }
    stack.push(name);
    let html = draw_chain(name, load, req, context, stack);
    stack.pop();
    html
}

/// Walk `{% extends %}`, then write the base template.
fn draw_chain<'a>(
    name: &'a str,
    load: fn(&str) -> Option<&'static str>,
    req: &Request<'_>,
    context: &[(&str, &str)],
    stack: &mut Vec<&'a str>,
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

    let static_url = if chain.iter().any(|parsed| uses_static(&parsed.nodes)) {
        req.static_url.ok_or("static files are not configured")?
    } else {
        ""
    };
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
    write(
        &mut out,
        &chain[base].nodes,
        &chosen,
        load,
        req,
        context,
        static_url,
        stack,
    )?;
    Ok(out)
}

/// True when any node, including inside a block, uses `{% static %}`.
fn uses_static(nodes: &[Node<'_>]) -> bool {
    nodes.iter().any(|node| match node {
        Node::Static(_) => true,
        Node::Block(_, body) => uses_static(body),
        Node::Text(_) | Node::Var(_) | Node::Include(_) => false,
    })
}

/// Write HTML. A child block replaces the parent block of the same name.
fn write<'a>(
    out: &mut String,
    nodes: &[Node<'a>],
    blocks: &[(&str, &[Node<'a>])],
    load: fn(&str) -> Option<&'static str>,
    req: &Request<'_>,
    context: &[(&str, &str)],
    static_url: &str,
    stack: &mut Vec<&'a str>,
) -> Result<(), String> {
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Var(name) => push_escaped(out, lookup(name, req, context)),
            Node::Static(name) => {
                push_escaped(out, static_url);
                push_escaped(out, name);
            }
            Node::Include(name) => {
                let html = draw_at(name, load, req, context, stack)?;
                out.push_str(&html);
            }
            Node::Block(name, body) => {
                let inner = blocks
                    .iter()
                    .find(|(found, _)| found == name)
                    .map(|(_, body)| *body)
                    .unwrap_or(body);
                write(out, inner, &[], load, req, context, static_url, stack)?;
            }
        }
    }
    Ok(())
}

/// A context value, or `request.path` and `request.method`. A missing name is empty.
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

/// Escape `& < > " '` so a value cannot become markup.
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
#[path = "../tests/template.rs"]
mod tests;
