//! The newsletter sign-up: plain HTML forms on this origin, relayed
//! server-side to grund insights in-cluster (its docs/design/newsletter.md).
//!
//! The browser only ever talks to grund.sh. Every POST answers 303 to a
//! static page (thanks, error, expired, unsubscribed) or to a page rendered
//! here with its token, and never 500: when insights is down or slow (3 s),
//! the visitor sees the error page and nothing else on the site is affected.
//!
//! The mailed link is a GET that shows a button; only the button's POST
//! confirms, because mail scanners open links on their own. A form posted
//! from another origin is refused. The honeypot and rate limit are judged by
//! insights, which sees the client address from the configured header.
//! Nothing here logs a form body or an address.

use std::time::Duration;

use axum::{
    body::Bytes,
    extract,
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use http_body_util::{BodyExt, Full};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use percent_encoding::percent_decode_str;

use crate::state::State;

/// The one limit on a form body. The form has four short fields.
pub const BODY_LIMIT: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_secs(3);

const CONFIRM_PAGE: &str = include_str!("newsletter/confirm.html");
const CONFIRMED_PAGE: &str = include_str!("newsletter/confirmed.html");
const UNSUBSCRIBE_PAGE: &str = include_str!("newsletter/unsubscribe.html");

pub const THANKS: &str = "/newsletter/thanks";
pub const ERROR: &str = "/newsletter/error";
pub const EXPIRED: &str = "/newsletter/expired";
pub const UNSUBSCRIBED: &str = "/newsletter/unsubscribed";

type HttpClient = Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>;

/// The relay to insights. Present only when GRUND_WEBSITE_NEWSLETTER is on.
#[derive(Clone)]
pub struct Relay {
    base: String,
    token: Option<String>,
    site: String,
    origin: String,
    client_ip_header: Option<String>,
    client: HttpClient,
}

impl Relay {
    pub fn new(
        base: &str,
        token: Option<String>,
        site: String,
        origin: String,
        client_ip_header: Option<String>,
    ) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            token,
            site,
            origin,
            client_ip_header,
            client: Client::builder(TokioExecutor::new()).build_http(),
        }
    }

    /// POSTs JSON to insights. `None` when it did not answer in time.
    async fn post(&self, path: &str, body: serde_json::Value) -> Option<(u16, serde_json::Value)> {
        let uri: hyper::Uri = format!("{}{path}", self.base).parse().ok()?;
        let mut request = hyper::Request::post(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::USER_AGENT, "grund-website");
        if let Some(token) = &self.token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let request = request
            .body(Full::new(Bytes::from(body.to_string())))
            .ok()?;
        let exchange = async {
            let response = self.client.request(request).await.ok()?;
            let status = response.status().as_u16();
            let bytes = response.into_body().collect().await.ok()?.to_bytes();
            Some((
                status,
                serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
            ))
        };
        tokio::time::timeout(TIMEOUT, exchange).await.ok().flatten()
    }

    /// A form must come from this origin. Browsers always send Origin on a
    /// POST; a request without one (a mail provider's one-click unsubscribe)
    /// is let through.
    fn same_origin(&self, headers: &HeaderMap) -> bool {
        headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .is_none_or(|origin| origin == self.origin)
    }
}

/// Reads an `application/x-www-form-urlencoded` body into (name, value)
/// pairs.
pub fn form(body: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(body);
    text.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let decode = |s: &str| {
                percent_decode_str(&s.replace('+', " "))
                    .decode_utf8_lossy()
                    .into_owned()
            };
            (decode(name), decode(value))
        })
        .collect()
}

fn field<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.trim().is_empty())
}

/// The path and utm tags of the page the form was on, from `Referer` (sent
/// in full for same-origin requests under strict-origin-when-cross-origin).
pub fn first_touch(referer: Option<&str>) -> (Option<String>, [Option<String>; 3]) {
    let Some(rest) = referer
        .and_then(|r| r.split_once("://"))
        .map(|(_, rest)| rest)
    else {
        return (None, [None, None, None]);
    };
    let path_and_query = rest.find('/').map_or("/", |at| &rest[at..]);
    let (path, query) = path_and_query
        .split_once('?')
        .unwrap_or((path_and_query, ""));
    let path = path.split('#').next().unwrap_or("/").to_string();
    let tags = form(query.split('#').next().unwrap_or("").as_bytes());
    let tag = |name: &str| field(&tags, name).map(|v| v.chars().take(100).collect());
    (
        Some(path),
        [tag("utm_source"), tag("utm_medium"), tag("utm_campaign")],
    )
}

/// A token as insights issues them: base64url, bounded.
fn valid_token(token: &str) -> bool {
    (16..=128).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn see_other(location: &str) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(location) {
        headers.insert(header::LOCATION, value);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// A page rendered with its token. The token is checked to be base64url
/// before it is placed, so it needs no escaping.
fn page_with(template: &str, token: &str) -> Response {
    let mut response = (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        template.replace("{{token}}", token),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn relay(state: &State) -> Option<&Relay> {
    state.newsletter.as_ref()
}

/// The `token` query parameter.
fn query_token(uri: &Uri) -> Option<String> {
    let fields = form(uri.query().unwrap_or("").as_bytes());
    field(&fields, "token").map(str::to_string)
}

/// `POST /newsletter`: the sign-up form.
pub async fn subscribe(
    extract::State(state): extract::State<State>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(relay) = relay(&state) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !relay.same_origin(&headers) {
        return (
            StatusCode::FORBIDDEN,
            "This form was posted from another site.",
        )
            .into_response();
    }
    let fields = form(&body);
    let header_value = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let (landing_path, [source, medium, campaign]) = first_touch(header_value("referer"));
    let client_ip = relay
        .client_ip_header
        .as_deref()
        .and_then(header_value)
        .map(|v| v.split(',').next().unwrap_or("").trim().to_string());
    let consent = field(&fields, "consent");
    let request = serde_json::json!({
        "site": relay.site,
        "email": field(&fields, "email").unwrap_or(""),
        "consent": { "given": consent.is_some(), "text_version": consent.unwrap_or("newsletter-2026-10") },
        "website": field(&fields, "website"),
        "client_ip": client_ip,
        "landing_path": landing_path,
        "utm_source": source,
        "utm_medium": medium,
        "utm_campaign": campaign,
    });
    match relay.post("/v1/newsletter/subscriptions", request).await {
        Some((202, _)) => see_other(THANKS),
        Some((status, _)) => {
            tracing::info!(status, "insights did not accept a newsletter sign-up");
            see_other(ERROR)
        }
        None => {
            tracing::warn!("insights did not answer a newsletter sign-up in time");
            see_other(ERROR)
        }
    }
}

/// `GET /newsletter/confirm?token=`: the mailed link. Shows a button; does
/// not confirm.
pub async fn confirm_page(extract::State(state): extract::State<State>, uri: Uri) -> Response {
    if relay(&state).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match query_token(&uri).as_deref().filter(|t| valid_token(t)) {
        Some(token) => page_with(CONFIRM_PAGE, token),
        None => see_other(EXPIRED),
    }
}

/// `POST /newsletter/confirm`: the button.
pub async fn confirm(
    extract::State(state): extract::State<State>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(relay) = relay(&state) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !relay.same_origin(&headers) {
        return (
            StatusCode::FORBIDDEN,
            "This form was posted from another site.",
        )
            .into_response();
    }
    let fields = form(&body);
    let Some(token) = field(&fields, "token").filter(|t| valid_token(t)) else {
        return see_other(EXPIRED);
    };
    match relay
        .post(
            "/v1/newsletter/confirm",
            serde_json::json!({ "token": token }),
        )
        .await
    {
        Some((200, body)) => match body["unsubscribe_token"]
            .as_str()
            .filter(|t| valid_token(t))
        {
            Some(unsubscribe) => see_other(&format!("/newsletter/confirmed?token={unsubscribe}")),
            None => see_other(ERROR),
        },
        Some((404, _)) => see_other(EXPIRED),
        _ => see_other(ERROR),
    }
}

/// `GET /newsletter/confirmed?token=`: done, with the unsubscribe link.
pub async fn confirmed_page(extract::State(state): extract::State<State>, uri: Uri) -> Response {
    if relay(&state).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match query_token(&uri).as_deref().filter(|t| valid_token(t)) {
        Some(token) => page_with(CONFIRMED_PAGE, token),
        None => see_other("/"),
    }
}

/// `GET /newsletter/unsubscribe?token=`: shows a button.
pub async fn unsubscribe_page(extract::State(state): extract::State<State>, uri: Uri) -> Response {
    if relay(&state).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match query_token(&uri).as_deref().filter(|t| valid_token(t)) {
        Some(token) => page_with(UNSUBSCRIBE_PAGE, token),
        None => see_other(EXPIRED),
    }
}

/// `POST /newsletter/unsubscribe`: the button, or a mail provider's RFC 8058
/// one-click POST (`?token=` with `List-Unsubscribe=One-Click` in the body).
pub async fn unsubscribe(
    extract::State(state): extract::State<State>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(relay) = relay(&state) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !relay.same_origin(&headers) {
        return (
            StatusCode::FORBIDDEN,
            "This form was posted from another site.",
        )
            .into_response();
    }
    let fields = form(&body);
    let token = field(&fields, "token")
        .map(str::to_string)
        .or_else(|| query_token(&uri))
        .filter(|t| valid_token(t));
    let Some(token) = token else {
        return see_other(EXPIRED);
    };
    match relay
        .post(
            "/v1/newsletter/unsubscribe",
            serde_json::json!({ "token": token }),
        )
        .await
    {
        Some((200, _)) => see_other(UNSUBSCRIBED),
        Some((404, _)) => see_other(EXPIRED),
        _ => see_other(ERROR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_body_decodes_plus_and_percent() {
        let fields =
            form(b"email=ada%40example.com&consent=newsletter-2026-10&website=&name=Ada+Example");
        assert_eq!(field(&fields, "email"), Some("ada@example.com"));
        assert_eq!(field(&fields, "name"), Some("Ada Example"));
        assert_eq!(field(&fields, "website"), None);
    }

    #[test]
    fn the_first_touch_is_the_form_pages_path_and_utm_tags() {
        let (path, [source, medium, campaign]) = first_touch(Some(
            "https://grund.sh/blog/?utm_source=HN&utm_campaign=launch#x",
        ));
        assert_eq!(path.as_deref(), Some("/blog/"));
        assert_eq!(source.as_deref(), Some("HN"));
        assert_eq!(medium, None);
        assert_eq!(campaign.as_deref(), Some("launch"));
        assert_eq!(
            first_touch(Some("https://grund.sh")).0.as_deref(),
            Some("/")
        );
        assert_eq!(first_touch(None).0, None);
    }

    #[test]
    fn only_base64url_tokens_are_placed_in_a_page() {
        assert!(valid_token("AbC-_0123456789abcdefghij"));
        assert!(!valid_token("short"));
        assert!(!valid_token("\"><script>alert(1)</script>xxxxxxxx"));
    }

    #[test]
    fn the_rendered_pages_need_nothing_the_csp_forbids() {
        for page in [CONFIRM_PAGE, CONFIRMED_PAGE, UNSUBSCRIBE_PAGE] {
            let html = page.to_ascii_lowercase();
            assert!(html.contains("{{token}}"));
            assert!(
                !html.contains("<script") && !html.contains("<style") && !html.contains(" style=")
            );
            assert!(html.contains("noindex"));
        }
    }
}
