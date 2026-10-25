//! Jango is a small Django-style web framework.
//!
//! A view is a function. Paths live in one router.
//! [`run`](crate::run) starts the server.

// -----------------------------------------------
// Public API
// Stage 1 is requests, responses, URL patterns,
// and the development server.
// -----------------------------------------------

mod http;
mod parse;
mod router;
mod server;
mod sys;

pub use http::{Method, Request, Response};
pub use router::Router;
pub use server::run;

/// Names you want in `urls.rs` and `views.rs`.
pub mod prelude {
    pub use crate::{routes, Method, Request, Response, Router};
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
/// use jango::prelude::*;
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
