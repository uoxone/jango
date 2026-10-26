use super::*;
use crate::http::Method;

fn load(name: &str) -> Option<&'static str> {
    Some(match name {
        "shell.html" => {
            "a { color: red }\n<title>{% block title %}{% endblock %}</title>\n{% block content %}{% endblock %}"
        }
        "home.html" => {
            "{% extends \"shell.html\" %}\n{% block title %}Home{% endblock %}\n{% block content %}<h1>{{ name }}</h1>{% endblock %}"
        }
        "about.html" => {
            "{% extends \"shell.html\" %}{% block content %}{{ request.path }} {{ request.method }}{% endblock %}"
        }
        "plain.html" => "Hello {# secret #}{{ name }}",
        "base.html" => "P{% block a %}A{% endblock %}{% block b %}B{% endblock %}",
        "child.html" => "{% extends \"base.html\" %}LOST{% block a %}N{% endblock %}",
        "mid.html" => "{% extends \"base.html\" %}{% block a %}M{% endblock %}",
        "top.html" => "{% extends \"mid.html\" %}{% block a %}T{% endblock %}",
        "only.html" => "{% block title %}Hi {{ name }}{% endblock %}",
        "sq.html" => "{% extends 'base.html' %}{% block a %}Q{% endblock %}",
        "note.html" => "{# hi #}\n{% extends \"base.html\" %}{% block b %}C{% endblock %}",
        "spaced.html" => "{%   extends   \"base.html\"   %}{%  block   a  %}S{%  endblock   %}",
        "tight.html" => "{{request.path}}{{ missing }}",
        "loop.html" => "{% extends \"loop.html\" %}",
        "cycle.html" => "{% extends \"other.html\" %}",
        "other.html" => "{% extends \"cycle.html\" %}",
        "bad.html" => "{% if x %}{% endif %}",
        "open.html" => "{{ name",
        "badvar.html" => "{{ 1name }}",
        "deepvar.html" => "{{ a.b.c }}",
        "mismatch.html" => "{% block a %}{% endblock b %}",
        "extraend.html" => "{% endblock %}",
        "unclosed.html" => "{% block a %}hi",
        "late.html" => "x{% extends \"base.html\" %}",
        "emptyname.html" => "{% extends \"\" %}",
        "bare.html" => "{% extends page %}",
        "badblock.html" => "{% block 1 %}",
        "novar.html" => "{{ }}",
        "opencom.html" => "{# hi",
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
    assert_eq!(res.content_type, "text/html; charset=utf-8");
    assert!(page.contains("a { color: red }"));
    assert!(page.contains("<title>Home</title>"));
    assert!(page.contains("<h1>&lt;b&gt;&quot;&amp;&#x27;</h1>"));
    assert!(!page.contains("<b>"));
}

#[test]
fn request_fields_comments_and_blocks() {
    let req = sample("/about");
    assert_eq!(
        body(&render(&req, "about.html", &[])),
        "a { color: red }\n<title></title>\n/about GET"
    );
    assert_eq!(
        body(&render(&req, "plain.html", &[("name", "Ada")])),
        "Hello Ada"
    );

    let req = sample("/");
    assert_eq!(body(&render(&req, "child.html", &[])), "PNB");
    assert_eq!(body(&render(&req, "top.html", &[])), "PTB");
    assert_eq!(
        body(&render(&req, "only.html", &[("name", "Ada")])),
        "Hi Ada"
    );
    assert_eq!(body(&render(&req, "sq.html", &[])), "PQB");
    assert_eq!(body(&render(&req, "note.html", &[])), "PAC");
    assert_eq!(body(&render(&req, "spaced.html", &[])), "PSB");
    assert_eq!(body(&render(&sample("/z"), "tight.html", &[])), "/z");

    let bare = crate::render!(&req, "plain.html");
    assert_eq!(body(&bare), "Hello ");
    let named = crate::render!(&req, "plain.html", { "name" => "Ada" });
    assert_eq!(body(&named), "Hello Ada");
}

#[test]
fn template_errors() {
    let req = sample("/");
    let empty: &[(&str, &str)] = &[];
    for (name, needle) in [
        ("missing.html", "was not found"),
        ("loop.html", "extends itself"),
        ("cycle.html", "extends itself"),
        ("bad.html", "unknown tag"),
        ("open.html", "unclosed"),
        ("opencom.html", "unclosed"),
        ("badvar.html", "invalid variable"),
        ("deepvar.html", "invalid variable"),
        ("novar.html", "invalid variable"),
        ("mismatch.html", "does not match"),
        ("extraend.html", "unexpected endblock"),
        ("unclosed.html", "unclosed block"),
        ("late.html", "first tag"),
        ("emptyname.html", "invalid template name"),
        ("bare.html", "must be quoted"),
        ("badblock.html", "invalid block"),
    ] {
        let res = render(&req, name, empty);
        assert_eq!(res.status, 500, "{name}");
        assert!(body(&res).contains(needle), "{name}: {}", body(&res));
    }

    let lost = Request::new(Method::Get, "/", "", &[], b"");
    let res = render(&lost, "plain.html", empty);
    assert_eq!(res.status, 500);
    assert!(body(&res).contains("not loaded"));
}

#[test]
fn extend_depth_is_capped() {
    const CHAIN: [&str; 17] = [
        "ok",
        "{% extends \"0\" %}",
        "{% extends \"1\" %}",
        "{% extends \"2\" %}",
        "{% extends \"3\" %}",
        "{% extends \"4\" %}",
        "{% extends \"5\" %}",
        "{% extends \"6\" %}",
        "{% extends \"7\" %}",
        "{% extends \"8\" %}",
        "{% extends \"9\" %}",
        "{% extends \"10\" %}",
        "{% extends \"11\" %}",
        "{% extends \"12\" %}",
        "{% extends \"13\" %}",
        "{% extends \"14\" %}",
        "{% extends \"15\" %}",
    ];
    fn deep(name: &str) -> Option<&'static str> {
        let n: usize = name.parse().ok()?;
        CHAIN.get(n).copied()
    }

    let mut req = sample("/");
    req.templates = Some(deep);
    assert_eq!(body(&render(&req, "15", &[])), "ok");
    let res = render(&req, "16", &[]);
    assert_eq!(res.status, 500);
    assert!(body(&res).contains("too deep"));
}

#[test]
fn project_templates_render() {
    fn project(name: &str) -> Option<&'static str> {
        Some(match name {
            "page.html" => include_str!("../templates/page.html.in"),
            "home.html" => include_str!("../templates/home.html.in"),
            "about.html" => include_str!("../templates/about.html.in"),
            _ => return None,
        })
    }

    let mut req = Request::new(Method::Get, "/about", "", &[], b"");
    req.templates = Some(project);
    let home = render(&req, "home.html", &[("addr", "127.0.0.1:8000")]);
    let page = body(&home);
    assert_eq!(home.status, 200, "{page}");
    assert!(page.starts_with("<!DOCTYPE html>"));
    assert!(page.contains("<title>__NAME__ · OnGo</title>"));
    assert!(page.contains("<h1>__NAME__</h1>"));
    assert!(page.contains("127.0.0.1:8000"));
    assert!(page.contains("--paper: #f6f1e8;"));
    assert!(!page.contains("{{ addr }}"));
    assert!(!page.contains("{% block"));

    let about = body(&render(&req, "about.html", &[]));
    assert!(about.contains("<title>About · __NAME__ · OnGo</title>"));
    assert!(about.contains("<code>/about</code>"));
    assert!(about.contains("← Home"));
}
