# Jango

A tiny Django-style web framework for Rust. 🧡

Stage 1 is the fun part: projects, URLs, views, and a fast little server. The whole stack lives in this repo. The standard library is the only dependency. No crates from outside.

## 🚀 Git Commit Guidelines

- ✨ feat: for new features
- 🔥 fix: for bug fixes
- ♻️ refactor: for code refactoring that does not add features or fix bugs
- 🗑️ remove: remove codes or packages

## ✨ Start a project

From this folder, once:

```bash
cargo install --path .
```

Then the good part:

```bash
jango create project nava
jango run nava
```

`jango run` is the cozy dev loop. It compiles quickly so you can keep editing views.

Want the fast binary?

```bash
jango build nava
```

Prefer to skip the install? This works too:

```bash
cargo run -- create project nava
cargo run -- run nava
```

## 🎵 Write something lovely

`src/urls.rs` is the whole map. One line, one page.

```rust
pub fn urlpatterns() -> Router {
    routes! {
        "/" => views::home,
        "/about" => views::about,
        "/song/<id>" => views::song,
    }
}
```

`src/views.rs` is just functions. Request in, response out. HTML lives in `templates/`.

```rust
pub fn home(req: &Request) -> Response {
    render!(req, "home.html", {
        "addr" => settings::ADDR,
    })
}

pub fn song(req: &Request) -> Response {
    Response::text(req.param("id").unwrap_or(""))
}
```

`{{ addr }}` and `{{ request.path }}` are escaped. `{% extends "page.html" %}` and `{% block content %}` share the page shell.

Paths are exact, so `/about` and `/about/` are two different doors. `<id>` catches one piece of the path, like `/song/12`.

Handy helpers:

- `render!(req, "home.html")`
- `Response::html`, `text`, `json`, `status`, `redirect`
- `req.method()`, `req.query("q")`, `req.body_str()`, `req.param("id")`

## 📦 Ship it

Yes. The server does not need Cargo or rustc.

```bash
jango build nava
```

Upload this one file and run it:

```text
nava/target/release/nava
```

```bash
./nava
```

Before you upload, set `ADDR` in `src/settings.rs` to `0.0.0.0:8000` so the world can knock. 🌍

The binary links to the system libc, which Linux already has. Build it on a machine whose glibc is close to the server's, and you are done.

Your laptop still uses Cargo to compile. The server only runs the file.

## ⚡ Speed

Jango speaks HTTP itself: `epoll`, one thread per core, `SO_REUSEPORT`. A small response walks a short path.

On this machine, a small release build did about:

- 🎯 **400,000** requests/sec with 8 keep-alive connections
- 🚀 **975,000** requests/sec when requests were sent in a batch

Keep views light. They run on the same thread that holds the connection.

## 🧪 Run the tests

From this folder:

```bash
cargo test
```

You get the framework tests, the `jango` command tests, and the doc test. A good run ends in `ok`.

A smaller slice:

```bash
cargo test --lib           # framework
cargo test --bin jango     # create, run, build
cargo test template        # tests with template in the name
cargo test -- --list       # names only
```

The tests live in `core/tests/`, one file per module. One of them listens on `127.0.0.1`. The rest stay in the process.

## 🌱 Stage 1

Already here, and happy about it:

- 📁 `create`, `run`, and `build`
- 🧭 exact paths and `/song/<id>`
- 📬 `GET` `POST` `PUT` `PATCH` `DELETE` `HEAD` `OPTIONS`
- 🔗 `Content-Length` bodies and keep-alive
- 📄 `templates/` with `{{ name }}`, `{% block %}`, and `{% extends %}`

Saved for a later stage: a database, admin, middleware, chunked bodies, and IPv6.
