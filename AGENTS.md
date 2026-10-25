# Ongo

Ongo is a small Django-style web framework written in Rust. Stage 1 is projects, URLs, views, and the HTTP server.

## Code

- Keep code short, clean, and idiomatic Rust.
- Put a section comment on each part. Use this shape:

```rust
// -----------------------------------------------
// Title
// What this part does, in one or two lines.
// -----------------------------------------------
```

- App code must stay pleasant to write. A view is a function. `urls.rs` is a short list of paths:

```rust
pub fn urlpatterns() -> Router {
    routes! {
        "/" => views::home,
        "/song/<id>" => views::song,
    }
}
```

- The sample project name is `nava`. Do not bring back `food`.

## Dependencies

- Never add a crate from outside this repository. No crates.io packages.
- Use only the Rust standard library and code written inside Ongo.
- Do not install Axum, Tokio, Hyper, Serde, or any other library to compare, test, or “help”.

## Speed and shipping

- Keep the server faster than Axum by staying small: no Tower, no Hyper, no middleware stack.
- `ongo create project <name>`, `ongo run <name>`, and `ongo build <name>` are the commands.
- `ongo build` must produce one executable. That file runs on a server with no Cargo and no rustc. It may link the system libc.
- The `ongo` command on disk is `~/.cargo/bin/ongo`. Editing `core/src/main.rs` does nothing until `cargo install --path .`.

## Scope

- Do not add a database, admin, template engine, middleware, chunked bodies, or IPv6 unless the user asks.
- Keep command output short.
