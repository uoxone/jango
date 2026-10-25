use super::*;

fn home(req: &Request) -> Response {
    Response::text(req.path())
}

fn song(req: &Request) -> Response {
    Response::text(req.param("id").unwrap_or(""))
}

#[test]
fn routes_macro_builds_a_router() {
    let app = routes! {
        "/" => home,
        "/song/<id>" => song,
    };
    let mut req = Request::new(Method::Get, "/song/4", "", &[], b"");
    let res = app.call(&mut req);
    assert_eq!(res.status, 200);
    assert_eq!(res.body, b"4");

    let empty = routes! {};
    let mut req = Request::new(Method::Get, "/", "", &[], b"");
    assert_eq!(empty.call(&mut req).status, 404);
}
