//! Ongo is a small Django-style web framework.
//!
//! A view is a function. Paths live in one router.
//! Templates use `{{ name }}`, `{% block %}`, `{% extends %}`, `{% include %}`, and `{% static %}`.
//! Files in `static/` are served at `STATIC_URL`.
//! [`run`](crate::run) starts the server.

// -----------------------------------------------
// Public API
// Requests, responses, URL patterns, templates,
// and the development server.
// -----------------------------------------------

mod http;
mod parse;
mod router;
mod server;
mod staticfiles;
mod sys;
mod template;

pub use http::{Method, Request, Response};
pub use router::Router;
pub use server::run;

// `render!` calls this. Hidden so views use the macro.
#[doc(hidden)]
pub use template::render as __render;

/// Names you want in `urls.rs` and `views.rs`.
pub mod prelude {
    pub use crate::{render, routes, Method, Request, Response, Router};
}

// -----------------------------------------------
// routes!
// One page of URLs, the same idea as Django's
// urlpatterns. A segment written as <id> captures
// that piece of the path.
// -----------------------------------------------

/// Build a [`Router`] from `"path" => view` pairs.
///
/// ```
/// use ongo::prelude::*;
///
/// fn home(_req: &Request) -> Response {
///     Response::text("hello")
/// }
///
/// let _app = routes! {
///     "/" => home,
/// };
/// ```
#[macro_export]
macro_rules! routes {
    ($($path:literal => $view:path),* $(,)?) => {
        $crate::Router::new()
            $(.route($path, $view))*
    };
}

#[cfg(test)]
#[path = "../tests/routes.rs"]
mod tests;
