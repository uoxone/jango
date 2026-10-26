//! Tests for the static folder.
//!
//! Content types, paths that must stay inside the folder, and byte ranges.

use super::*;
use crate::http::Method;
use crate::Router;
use std::fs;
use std::path::Path;

fn home(req: &Request) -> Response {
    Response::text(req.path())
}

fn gone(req: &Request) -> Response {
    Response::status(410, req.path())
}

fn root() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ongo-static-{}-{}", std::process::id(), unique()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("css")).unwrap();
    fs::write(dir.join("css/site.css"), b"body{color:red}").unwrap();
    fs::write(dir.join("cover.JPG"), b"jpg").unwrap();
    fs::write(dir.join("clip.mp4"), b"0123456789").unwrap();
    fs::write(dir.join("notes.dat"), b"raw").unwrap();
    fs::write(dir.join("my file.txt"), b"hello").unwrap();
    fs::create_dir_all(dir.join("empty")).unwrap();
    dir
}

fn unique() -> u32 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn call(dir: &Path, path: &str) -> Response {
    ask(dir, Method::Get, path, &[])
}

fn ask(dir: &Path, method: Method, path: &str, headers: &[(&str, &str)]) -> Response {
    let router = Router::new()
        .route("/", home)
        .route("/about", home)
        .route("/static/css/site.css", home)
        .not_found(gone)
        .staticfiles("/static/", dir);
    let mut req = Request::new(method, path, "", headers, b"");
    router.call(&mut req)
}

#[test]
fn serves_css_images_video_and_other_files() {
    let dir = root();
    let css = call(&dir, "/static/css/site.css");
    assert_eq!(css.status, 200);
    assert_eq!(css.content_type, "text/css; charset=utf-8");
    assert!(css.accept_ranges);
    assert_eq!(css.file.as_ref().unwrap().len, 15);
    assert!(css.body.is_empty());

    assert_eq!(call(&dir, "/static/cover.JPG").content_type, "image/jpeg");
    assert_eq!(call(&dir, "/static/clip.mp4").content_type, "video/mp4");
    assert_eq!(
        call(&dir, "/static/notes.dat").content_type,
        "application/octet-stream"
    );

    let named = call(&dir, "/static/my%20file.txt");
    assert_eq!(named.status, 200);
    assert_eq!(named.content_type, "text/plain; charset=utf-8");
    assert_eq!(fs::read(named.file.unwrap().path).unwrap(), b"hello");

    assert_eq!(call(&dir, "/").body, b"/");
    assert_eq!(call(&dir, "/about").body, b"/about");
    assert_eq!(call(&dir, "/gone").status, 410);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_unsafe_and_foreign_paths_stay_out() {
    let dir = root();
    let outside =
        std::env::temp_dir().join(format!("ongo-secret-{}-{}", std::process::id(), unique()));
    fs::write(&outside, b"secret").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("leak")).unwrap();

    for path in [
        "/static/missing.css",
        "/static",
        "/static/",
        "/static/css",
        "/static/empty",
        "/static/css/../cover.JPG",
        "/static/%2e%2e/leak",
        "/static/css/%2e%2e/%2e%2e/leak",
        "/static/%2fcover.JPG",
        "/static/leak",
        "/static/%00.css",
        "/staticish",
    ] {
        let res = call(&dir, path);
        assert!(res.file.is_none(), "{path}");
        if path == "/staticish" {
            assert_eq!(res.status, 410, "{path}");
        } else {
            assert_eq!(res.status, 404, "{path}");
            assert!(
                !res.body.windows(6).any(|bytes| bytes == b"secret"),
                "{path}"
            );
        }
    }

    assert_eq!(
        ask(&dir, Method::Post, "/static/css/site.css", &[]).status,
        405
    );
    assert_eq!(
        ask(&dir, Method::Post, "/static/missing.css", &[]).status,
        405
    );

    let media = Router::new().staticfiles("/media/", &dir);
    let mut req = Request::new(Method::Get, "/media/clip.mp4", "", &[], b"");
    let res = media.call(&mut req);
    assert_eq!(res.status, 200);
    assert_eq!(res.content_type, "video/mp4");

    std::os::unix::fs::symlink("css/site.css", dir.join("alias.css")).unwrap();
    let alias = call(&dir, "/static/alias.css");
    assert_eq!(alias.status, 200, "a link that stays inside the folder");
    assert_eq!(alias.file.as_ref().unwrap().len, 15);

    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_file(&outside);
}

#[test]
fn one_byte_range_and_a_refusal() {
    let dir = root();
    let full = call(&dir, "/static/clip.mp4");
    assert_eq!(full.status, 200);
    assert!(full.content_range.is_none());
    assert_eq!(full.file.as_ref().unwrap().len, 10);

    let part = ask(
        &dir,
        Method::Get,
        "/static/clip.mp4",
        &[("Range", "bytes=2-5")],
    );
    assert_eq!(part.status, 206);
    assert_eq!(part.content_range.as_deref(), Some("bytes 2-5/10"));
    let file = part.file.unwrap();
    assert_eq!((file.start, file.len), (2, 4));

    let tail = ask(
        &dir,
        Method::Get,
        "/static/clip.mp4",
        &[("Range", "bytes=8-")],
    );
    assert_eq!(tail.file.unwrap().start, 8);
    let suffix = ask(
        &dir,
        Method::Get,
        "/static/clip.mp4",
        &[("Range", "bytes=-3")],
    );
    let suffix = suffix.file.unwrap();
    assert_eq!((suffix.start, suffix.len), (7, 3));

    let wide = ask(
        &dir,
        Method::Get,
        "/static/clip.mp4",
        &[("Range", "bytes=0-99")],
    );
    assert_eq!(wide.status, 206);
    assert_eq!(wide.content_range.as_deref(), Some("bytes 0-9/10"));

    let bad = ask(
        &dir,
        Method::Get,
        "/static/clip.mp4",
        &[("Range", "bytes=20-30")],
    );
    assert_eq!(bad.status, 416);
    assert_eq!(bad.content_range.as_deref(), Some("bytes */10"));
    assert!(bad.file.is_none());

    let both = ask(
        &dir,
        Method::Get,
        "/static/clip.mp4",
        &[("Range", "bytes=0-1,2-3")],
    );
    assert_eq!(both.status, 200);
    assert_eq!(both.file.unwrap().len, 10);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_static_url_needs_a_slash_prefix() {
    for url in [
        "static/",
        "/static",
        "/",
        "static",
        "/static//",
        "/../",
        "/static/../",
    ] {
        let result = std::panic::catch_unwind(|| Router::new().staticfiles(url, "/tmp"));
        assert!(result.is_err(), "{url}");
    }
}
