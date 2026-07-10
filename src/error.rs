#![cfg_attr(target_arch = "wasm32", allow(unused))]
use std::error::Error as StdError;
use std::fmt;
use std::io;

use crate::{StatusCode, Url};

/// A `Result` alias where the `Err` case is `reqwest::Error`.
pub type Result<T> = std::result::Result<T, Error>;

/// The Errors that may occur when processing a `Request`.
///
/// Note: Errors may include the full URL used to make the `Request`. If the URL
/// contains sensitive information (e.g. an API key as a query parameter), be
/// sure to remove it ([`without_url`](Error::without_url))
pub struct Error {
    inner: Box<Inner>,
}

pub(crate) type BoxError = Box<dyn StdError + Send + Sync>;

struct Inner {
    kind: Kind,
    source: Option<BoxError>,
    url: Option<Url>,
}

impl Error {
    pub(crate) fn new<E>(kind: Kind, source: Option<E>) -> Error
    where
        E: Into<BoxError>,
    {
        Error {
            inner: Box::new(Inner {
                kind,
                source: source.map(Into::into),
                url: None,
            }),
        }
    }

    /// Returns a possible URL related to this error.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn run() {
    /// // displays last stop of a redirect loop
    /// let response = reqwest::get("http://site.with.redirect.loop").await;
    /// if let Err(e) = response {
    ///     if e.is_redirect() {
    ///         if let Some(final_stop) = e.url() {
    ///             println!("redirect loop at {final_stop}");
    ///         }
    ///     }
    /// }
    /// # }
    /// ```
    pub fn url(&self) -> Option<&Url> {
        self.inner.url.as_ref()
    }

    /// Returns a mutable reference to the URL related to this error
    ///
    /// This is useful if you need to remove sensitive information from the URL
    /// (e.g. an API key in the query), but do not want to remove the URL
    /// entirely.
    pub fn url_mut(&mut self) -> Option<&mut Url> {
        self.inner.url.as_mut()
    }

    /// Add a url related to this error (overwriting any existing)
    pub fn with_url(mut self, url: Url) -> Self {
        self.inner.url = Some(url);
        self
    }

    /// Strip the related url from this error (if, for example, it contains
    /// sensitive information)
    pub fn without_url(mut self) -> Self {
        self.inner.url = None;
        self
    }

    /// Returns true if the error is from a type Builder.
    pub fn is_builder(&self) -> bool {
        matches!(self.inner.kind, Kind::Builder)
    }

    /// Returns true if the error is from a `RedirectPolicy`.
    pub fn is_redirect(&self) -> bool {
        matches!(self.inner.kind, Kind::Redirect)
    }

    /// Returns true if the error is from `Response::error_for_status`.
    pub fn is_status(&self) -> bool {
        matches!(self.inner.kind, Kind::Status(_))
    }

    /// Returns true if the error is related to a timeout.
    pub fn is_timeout(&self) -> bool {
        let mut source = self.source();

        while let Some(err) = source {
            if err.is::<TimedOut>() {
                return true;
            }
            if let Some(io) = err.downcast_ref::<io::Error>() {
                if io.kind() == io::ErrorKind::TimedOut {
                    return true;
                }
            }
            source = err.source();
        }

        false
    }

    /// Returns true if the error is related to the request
    pub fn is_request(&self) -> bool {
        matches!(self.inner.kind, Kind::Request)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Returns true if the error is related to connect
    pub fn is_connect(&self) -> bool {
        let mut source = self.source();

        while let Some(err) = source {
            if let Some(hyper_err) = err.downcast_ref::<hyper_util::client::legacy::Error>() {
                if hyper_err.is_connect() {
                    return true;
                }
            }

            source = err.source();
        }

        false
    }

    /// Returns true if the error is related to the request or response body
    pub fn is_body(&self) -> bool {
        matches!(self.inner.kind, Kind::Body)
    }

    /// Returns true if the error is related to decoding the response's body
    pub fn is_decode(&self) -> bool {
        matches!(self.inner.kind, Kind::Decode)
    }

    /// Returns the status code, if the error was generated from a response.
    pub fn status(&self) -> Option<StatusCode> {
        match self.inner.kind {
            Kind::Status(code) => Some(code),
            _ => None,
        }
    }

    // private

    #[allow(unused)]
    pub(crate) fn into_io(self) -> io::Error {
        io::Error::new(io::ErrorKind::Other, self)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut builder = f.debug_struct("reqwest::Error");

        builder.field("kind", &self.inner.kind);

        if let Some(ref url) = self.inner.url {
            builder.field("url", &url.as_str());
        }
        if let Some(ref source) = self.inner.source {
            builder.field("source", source);
        }

        builder.finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.inner.kind {
            Kind::Builder => f.write_str("builder error")?,
            Kind::Request => f.write_str("error sending request")?,
            Kind::Body => f.write_str("request or response body error")?,
            Kind::Decode => f.write_str("error decoding response body")?,
            Kind::Redirect => f.write_str("error following redirect")?,
            Kind::Upgrade => f.write_str("error upgrading connection")?,
            Kind::Status(ref code) => {
                let prefix = if code.is_client_error() {
                    "HTTP status client error"
                } else {
                    debug_assert!(code.is_server_error());
                    "HTTP status server error"
                };
                write!(f, "{prefix} ({code})")?;
            }
        };

        if let Some(url) = &self.inner.url {
            write!(f, " for url ({url})")?;
        }

        Ok(())
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.inner.source.as_ref().map(|e| &**e as _)
    }
}

#[cfg(target_arch = "wasm32")]
impl From<crate::error::Error> for wasm_bindgen::JsValue {
    fn from(err: Error) -> wasm_bindgen::JsValue {
        js_sys::Error::from(err).into()
    }
}

#[cfg(target_arch = "wasm32")]
impl From<crate::error::Error> for js_sys::Error {
    fn from(err: Error) -> js_sys::Error {
        js_sys::Error::new(&format!("{err}"))
    }
}

#[derive(Debug)]
pub(crate) enum Kind {
    Builder,
    Request,
    Redirect,
    Status(StatusCode),
    Body,
    Decode,
    Upgrade,
}

// constructors

pub(crate) fn builder<E: Into<BoxError>>(e: E) -> Error {
    Error::new(Kind::Builder, Some(e))
}

pub(crate) fn body<E: Into<BoxError>>(e: E) -> Error {
    Error::new(Kind::Body, Some(e))
}

pub(crate) fn decode<E: Into<BoxError>>(e: E) -> Error {
    Error::new(Kind::Decode, Some(e))
}

pub(crate) fn request<E: Into<BoxError>>(e: E) -> Error {
    Error::new(Kind::Request, Some(e))
}

pub(crate) fn redirect<E: Into<BoxError>>(e: E, url: Url) -> Error {
    Error::new(Kind::Redirect, Some(e)).with_url(url)
}

pub(crate) fn status_code(url: Url, status: StatusCode) -> Error {
    Error::new(Kind::Status(status), None::<Error>).with_url(url)
}

pub(crate) fn url_bad_scheme(url: Url) -> Error {
    Error::new(Kind::Builder, Some(BadScheme)).with_url(url)
}

pub(crate) fn url_invalid_uri(url: Url) -> Error {
    Error::new(Kind::Builder, Some("Parsed Url is not a valid Uri")).with_url(url)
}

if_wasm! {
    /// Render a JS rejection (`JsValue`) as a clean error string.
    ///
    /// wasm-bindgen's default `Debug` for a `JsValue` holding a JS `Error`
    /// renders both `Error.toString()` and `Error.stack`; since both start with
    /// `<name>: <message>`, the message is duplicated and the whole thing is
    /// wrapped in a raw `JsValue(...)`. Render `<name>: <message>` once and the
    /// `.stack` once instead.
    ///
    /// For a browser `fetch()` failure the platform reports an opaque
    /// `TypeError: Failed to fetch` and deliberately does not expose the
    /// underlying reason (DNS, TLS, CORS, mixed-content). Append a hint so the
    /// user knows where to look. Skip the hint for other rejections (e.g.
    /// `AbortError`) so it is not misleading.
    pub(crate) fn wasm(js_val: wasm_bindgen::JsValue) -> BoxError {
        use wasm_bindgen::JsCast;

        if let Some(err) = js_val.dyn_ref::<js_sys::Error>() {
            let name = err.name().as_string().unwrap_or_default();
            let message = err.message().as_string().unwrap_or_default();
            // `name`/`message` are borrowed again below by `is_browser_fetch_failure`,
            // so the single-field arms clone (never move) and the empty case falls back
            // to a literal. `format!` only borrows, so the `(false, false)` arm is safe.
            let head = match (name.is_empty(), message.is_empty()) {
                (false, false) => format!("{name}: {message}"),
                (false, true) => name.clone(),
                (true, false) => message.clone(),
                _ => "Error".to_string(),
            };

            // V8 (Chrome/Edge) prefixes `Error.stack` with "<name>: <message>", so
            // appending the raw stack would print that line twice (once as `head`,
            // once as the stack's first line). SpiderMonkey (Firefox) does not.
            // When the stack already begins with `head`, use it verbatim (head +
            // frames, printed once); otherwise prepend `head`.
            let stack = js_sys::Reflect::get(&js_val, &wasm_bindgen::JsValue::from_str("stack"))
                .ok()
                .and_then(|v| v.as_string())
                .filter(|s| !s.is_empty());

            let mut out = match stack {
                Some(stack) if stack.starts_with(head.as_str()) => stack,
                Some(stack) => {
                    let mut s = head.clone();
                    s.push('\n');
                    s.push_str(&stack);
                    s
                }
                None => head.clone(),
            };

            if is_browser_fetch_failure(&name, &message) {
                out.push('\n');
                out.push_str(
                    "Note: browsers do not expose the underlying reason for a fetch failure. \
                     Common causes: server unreachable, mixed-content \
                     (https page -> http url), CORS rejection, or TLS/certificate error.",
                );
            }

            return out.into();
        }

        // Non-`Error` rejection (e.g. a string thrown from JS): no duplication
        // concern, fall back to the raw Debug.
        format!("{js_val:?}").into()
    }

    fn is_browser_fetch_failure(name: &str, message: &str) -> bool {
        (name == "TypeError" || name == "Error") && message.contains("Failed to fetch")
    }
}

pub(crate) fn upgrade<E: Into<BoxError>>(e: E) -> Error {
    Error::new(Kind::Upgrade, Some(e))
}

// io::Error helpers

#[cfg(any(
    feature = "gzip",
    feature = "zstd",
    feature = "brotli",
    feature = "deflate",
    feature = "blocking",
))]
pub(crate) fn into_io(e: BoxError) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e)
}

#[allow(unused)]
pub(crate) fn decode_io(e: io::Error) -> Error {
    if e.get_ref().map(|r| r.is::<Error>()).unwrap_or(false) {
        *e.into_inner()
            .expect("io::Error::get_ref was Some(_)")
            .downcast::<Error>()
            .expect("StdError::is() was true")
    } else {
        decode(e)
    }
}

// internal Error "sources"

#[derive(Debug)]
pub(crate) struct TimedOut;

impl fmt::Display for TimedOut {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("operation timed out")
    }
}

impl StdError for TimedOut {}

#[derive(Debug)]
pub(crate) struct BadScheme;

impl fmt::Display for BadScheme {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("URL scheme is not allowed")
    }
}

impl StdError for BadScheme {}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    #[test]
    fn test_source_chain() {
        let root = Error::new(Kind::Request, None::<Error>);
        assert!(root.source().is_none());

        let link = super::body(root);
        assert!(link.source().is_some());
        assert_send::<Error>();
        assert_sync::<Error>();
    }

    #[test]
    fn mem_size_of() {
        use std::mem::size_of;
        assert_eq!(size_of::<Error>(), size_of::<usize>());
    }

    #[test]
    fn roundtrip_io_error() {
        let orig = super::request("orig");
        // Convert reqwest::Error into an io::Error...
        let io = orig.into_io();
        // Convert that io::Error back into a reqwest::Error...
        let err = super::decode_io(io);
        // It should have pulled out the original, not nested it...
        match err.inner.kind {
            Kind::Request => (),
            _ => panic!("{err:?}"),
        }
    }

    #[test]
    fn from_unknown_io_error() {
        let orig = io::Error::new(io::ErrorKind::Other, "orly");
        let err = super::decode_io(orig);
        match err.inner.kind {
            Kind::Decode => (),
            _ => panic!("{err:?}"),
        }
    }

    #[test]
    fn is_timeout() {
        let err = super::request(super::TimedOut);
        assert!(err.is_timeout());

        let io = io::Error::new(io::ErrorKind::Other, err);
        let nested = super::request(io);
        assert!(nested.is_timeout());
    }
}
