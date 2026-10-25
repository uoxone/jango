use super::*;
use crate::http::Method;

fn home(req: &Request) -> Response {
    Response::text(req.path())
}

fn item(req: &Request) -> Response {
    Response::text(req.param("id").unwrap_or("?"))
}

fn call(router: &Router, path: &str) -> Response {
    let mut req = Request::new(Method::Get, path, "ignored", &[], b"");
    router.call(&mut req)
}

#[test]
fn static_routes_are_exact() {
    let router = Router::new().route("/", home).route("/about", home);
    assert_eq!(call(&router, "/").body, b"/");
    assert_eq!(call(&router, "/about").body, b"/about");
    assert_eq!(call(&router, "/about/").status, 404);
    assert_eq!(call(&router, "/About").status, 404);
    assert_eq!(call(&Router::default(), "/").status, 404);
}

#[test]
fn captures_one_segment() {
    fn named(req: &Request) -> Response {
        Response::text(format!(
            "{}:{}:{}",
            req.param("user").unwrap_or(""),
            req.param("id").unwrap_or(""),
            req.param("_id").unwrap_or("")
        ))
    }

    let songs = Router::new().route("/song/<id>", item);
    assert_eq!(call(&songs, "/song/12").body, b"12");
    assert_eq!(call(&songs, "/song/a-b_c").body, b"a-b_c");
    assert_eq!(call(&songs, "/song").status, 404);
    assert_eq!(call(&songs, "/song/").status, 404);
    assert_eq!(call(&songs, "/song/12/extra").status, 404);
    assert_eq!(call(&songs, "/other/12").status, 404);

    let router = Router::new()
        .route("/song/<id>", item)
        .route("/u/<user>/song/<id>", named)
        .route("/<_id>", named);
    assert_eq!(call(&router, "/u/ada/song/3").body, b"ada:3:");
    assert_eq!(call(&router, "/ab").body, b"::ab");
    assert_eq!(call(&router, "/song").body, b"::song");
}

#[test]
fn static_route_beats_a_pattern_and_the_first_pattern_wins() {
    fn by_name(req: &Request) -> Response {
        Response::text(format!("n:{}", req.param("name").unwrap_or("")))
    }

    let router = Router::new()
        .route("/song/live", home)
        .route("/song/<id>", item)
        .route("/<id>", item)
        .route("/<name>", by_name);
    assert_eq!(call(&router, "/song/live").body, b"/song/live");
    assert_eq!(call(&router, "/song/9").body, b"9");
    assert_eq!(call(&router, "/x").body, b"x");
}

#[test]
fn custom_not_found_and_a_full_map() {
    fn missing(req: &Request) -> Response {
        Response::status(410, req.path())
    }

    let paths = [
        "/", "/a", "/b", "/c", "/d", "/e", "/f", "/g", "/h", "/i", "/j", "/k", "/l", "/m", "/n",
        "/o", "/p", "/q", "/r",
    ];
    let mut router = Router::new().not_found(missing);
    for path in paths {
        router = router.route(path, home);
    }
    for path in paths {
        assert_eq!(call(&router, path).body, path.as_bytes(), "{path}");
    }
    let gone = call(&router, "/missing");
    assert_eq!(gone.status, 410);
    assert_eq!(gone.body, b"/missing");
}

#[test]
fn eight_captures() {
    fn eight(req: &Request) -> Response {
        let mut out = String::new();
        for name in ["a", "b", "c", "d", "e", "f", "g", "h"] {
            out.push_str(req.param(name).unwrap_or("?"));
        }
        Response::text(out)
    }

    let router = Router::new().route("/<a>/<b>/<c>/<d>/<e>/<f>/<g>/<h>", eight);
    assert_eq!(call(&router, "/1/2/3/4/5/6/7/8").body, b"12345678");
    assert_eq!(call(&router, "/1/2/3/4/5/6/7").status, 404);
}

#[test]
fn templates_reach_the_view() {
    fn page(name: &str) -> Option<&'static str> {
        (name == "t.html").then_some("Hello {{ name }}")
    }
    fn greet(req: &Request) -> Response {
        crate::render!(req, "t.html", { "name" => "Ada" })
    }

    let bare = Router::new().route("/", greet);
    assert_eq!(call(&bare, "/").status, 500);
    let ready = Router::new().route("/", greet).templates(page);
    let res = call(&ready, "/");
    assert_eq!(res.status, 200);
    assert_eq!(res.body, b"Hello Ada");
}

#[test]
fn bad_routes_panic() {
    for path in [
        "about",
        "/<id>/",
        "/a//<b>",
        "/song/<>",
        "/song/<a-b>",
        "/song/<id",
        "/<a>/<b>/<c>/<d>/<e>/<f>/<g>/<h>/<i>",
    ] {
        assert!(
            std::panic::catch_unwind(|| Router::new().route(path, home)).is_err(),
            "{path}"
        );
    }
    assert!(std::panic::catch_unwind(|| {
        let _ = Router::new().route("/", home).route("/", home);
    })
    .is_err());
    assert!(std::panic::catch_unwind(|| {
        let _ = Router::new().route("/<id>", home).route("/<id>", home);
    })
    .is_err());
}
