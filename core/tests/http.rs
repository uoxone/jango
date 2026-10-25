use super::*;

fn req<'a>(
    method: Method,
    path: &'a str,
    query: &'a str,
    headers: &'a [(&'a str, &'a str)],
    body: &'a [u8],
) -> Request<'a> {
    Request::new(method, path, query, headers, body)
}

#[test]
fn methods_round_trip() {
    for (text, method) in [
        ("GET", Method::Get),
        ("POST", Method::Post),
        ("PUT", Method::Put),
        ("DELETE", Method::Delete),
        ("HEAD", Method::Head),
        ("PATCH", Method::Patch),
        ("OPTIONS", Method::Options),
    ] {
        assert_eq!(Method::parse(text), Some(method));
        assert_eq!(method.as_str(), text);
        assert_eq!(method.to_string(), text);
    }
    assert_eq!(Method::parse("get"), None);
    assert_eq!(Method::parse(""), None);
    assert_eq!(Method::parse("CONNECT"), None);
}

#[test]
fn query_header_param_and_body() {
    let headers = [
        ("Host", "localhost"),
        ("Accept", "text/plain"),
        ("X-A", "1"),
        ("x-a", "2"),
    ];
    let mut sample = req(
        Method::Post,
        "/menu",
        "&q=a=b&page=2&flag&empty=&q=second",
        &headers,
        b"a=1",
    );
    sample.params[0] = ("id", "12");
    sample.params[1] = ("slug", "ada");
    sample.nparams = 2;

    assert_eq!(sample.method(), Method::Post);
    assert_eq!(sample.path(), "/menu");
    assert_eq!(sample.query("q"), Some("a=b"));
    assert_eq!(sample.query("page"), Some("2"));
    assert_eq!(sample.query("flag"), Some(""));
    assert_eq!(sample.query("empty"), Some(""));
    assert_eq!(sample.query("missing"), None);
    assert_eq!(sample.header("accept"), Some("text/plain"));
    assert_eq!(sample.header("HOST"), Some("localhost"));
    assert_eq!(sample.header("x-a"), Some("1"));
    assert_eq!(sample.header("missing"), None);
    assert_eq!(sample.param("id"), Some("12"));
    assert_eq!(sample.param("slug"), Some("ada"));
    assert_eq!(sample.param("nope"), None);
    assert_eq!(sample.body(), b"a=1");
    assert_eq!(sample.body_str(), Some("a=1"));

    assert_eq!(req(Method::Get, "/", "", &[], b"").query("q"), None);
    assert_eq!(req(Method::Get, "/", "", &[], b"\xff").body_str(), None);
    assert_eq!(
        req(Method::Get, "/", "q=a%20b", &[], b"").query("q"),
        Some("a%20b")
    );
}

#[test]
fn responses_and_reasons() {
    let text = Response::text("hi");
    assert_eq!(text.status, 200);
    assert_eq!(text.content_type, "text/plain; charset=utf-8");
    assert_eq!(text.body, b"hi");
    assert!(text.location.is_none());

    assert_eq!(
        Response::html("<p>").content_type,
        "text/html; charset=utf-8"
    );
    assert_eq!(
        Response::json("{}").content_type,
        "application/json; charset=utf-8"
    );

    let made = Response::status(201, "created");
    assert_eq!(made.status, 201);
    assert_eq!(made.body, b"created");

    let jump = Response::redirect("/next");
    assert_eq!(jump.status, 302);
    assert_eq!(jump.location.as_deref(), Some("/next"));
    assert!(jump.body.is_empty());

    for (code, phrase) in [
        (200, "OK"),
        (201, "Created"),
        (204, "No Content"),
        (301, "Moved Permanently"),
        (302, "Found"),
        (400, "Bad Request"),
        (404, "Not Found"),
        (405, "Method Not Allowed"),
        (413, "Payload Too Large"),
        (500, "Internal Server Error"),
        (501, "Not Implemented"),
        (418, "Response"),
    ] {
        assert_eq!(reason(code), phrase);
    }
}

#[test]
fn status_must_be_a_real_code() {
    for code in [0, 99, 600, 1000] {
        assert!(
            std::panic::catch_unwind(|| Response::status(code, "")).is_err(),
            "{code}"
        );
    }
    assert_eq!(Response::status(100, "").status, 100);
    assert_eq!(Response::status(599, "").status, 599);
}

#[test]
fn ascii_case_fold() {
    assert!(eq_ignore("Content-Length", "content-length"));
    assert!(eq_ignore("", ""));
    assert!(!eq_ignore("ab", "abc"));
    assert!(!eq_ignore("ab", "ac"));
}
