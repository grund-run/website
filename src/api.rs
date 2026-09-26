//! Routes and response headers. The decisions (which file, which encoding,
//! which cache policy, whether to redirect) live in `site.rs` and
//! `canonical.rs`; this file turns them into HTTP.
//!
//! Server-side rendering, when it comes, slots in here: minijinja page routes
//! registered before the `fallback`, rendered through one `Templates` type, and
//! the static table stays the fallback for everything else.

use axum::{
    Json, Router,
    body::Body,
    extract::{self, Request},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use tower_http::{
    catch_panic::CatchPanicLayer, set_header::SetResponseHeaderLayer, timeout::TimeoutLayer,
    trace::TraceLayer,
};

use crate::{
    canonical::CanonicalState,
    site::{self, Entry, Resolution, SiteState, if_none_match_hits},
    state::State,
};

/// Everything the page may load comes from this origin. No inline script or
/// style, no framing, no plugins. `img-src data:` admits inline SVG/PNG data
/// URIs, which carry no script. Loosening any directive is a reviewed code
/// change with its reason in the commit, never configuration.
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
    img-src 'self' data:; font-src 'self'; connect-src 'self'; manifest-src 'self'; \
    base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// Paths the server answers itself, ahead of the site. Site links may point
/// at them.
#[cfg(test)]
pub const SERVER_ROUTES: &[&str] = &[
    "/health/live",
    "/health/ready",
    SIGN_IN,
    "/newsletter",
    "/newsletter/confirm",
    "/newsletter/confirmed",
    "/newsletter/unsubscribe",
];

/// Sends visitors to the dashboard's login (GRUND_WEBSITE_APP_URL).
pub const SIGN_IN: &str = "/sign-in";

const PERMISSIONS_POLICY: &str =
    "camera=(), microphone=(), geolocation=(), payment=(), usb=(), browsing-topics=()";

/// A cached permanent redirect is hard to take back: browsers keep 301/308
/// indefinitely unless told otherwise. A day bounds a misconfiguration.
const REDIRECT_CACHE: &str = "public, max-age=86400";

pub fn router(state: State) -> Router {
    let config = state.config.clone();
    let noindex = config.noindex.then(|| {
        SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-robots-tag"),
            HeaderValue::from_static("noindex, nofollow"),
        )
    });
    let hsts = (config.hsts_max_age > 0).then(|| {
        SetResponseHeaderLayer::overriding(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::try_from(format!("max-age={}", config.hsts_max_age))
                .expect("a number is a valid header value"),
        )
    });

    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route(SIGN_IN, get(sign_in))
        .route(
            "/newsletter",
            post(crate::newsletter::subscribe)
                .layer(axum::extract::DefaultBodyLimit::max(crate::newsletter::BODY_LIMIT)),
        )
        .route(
            "/newsletter/confirm",
            get(crate::newsletter::confirm_page).post(crate::newsletter::confirm)
                .layer(axum::extract::DefaultBodyLimit::max(crate::newsletter::BODY_LIMIT)),
        )
        .route("/newsletter/confirmed", get(crate::newsletter::confirmed_page))
        .route(
            "/newsletter/unsubscribe",
            get(crate::newsletter::unsubscribe_page).post(crate::newsletter::unsubscribe)
                .layer(axum::extract::DefaultBodyLimit::max(crate::newsletter::BODY_LIMIT)),
        )
        .fallback(serve_site)
        .layer(middleware::from_fn_with_state(state.clone(), canonical_host))
        // Outside the canonical-host redirect, so alias redirects (308) are
        // seen and skipped; inside the timeout. Off unless configured.
        .layer(middleware::from_fn_with_state(state.clone(), crate::insights::capture))
        .with_state(state)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::SERVICE_UNAVAILABLE,
            config.request_timeout,
        ))
        .layer(CatchPanicLayer::new())
        // Outside the timeout and panic guard, so their responses carry the
        // headers too.
        .layer(tower::util::option_layer(noindex))
        .layer(tower::util::option_layer(hsts))
        .layer(header_layer(header::CONTENT_SECURITY_POLICY, CSP))
        .layer(header_layer(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(header_layer(header::REFERRER_POLICY, "strict-origin-when-cross-origin"))
        .layer(header_layer(header::X_FRAME_OPTIONS, "DENY"))
        .layer(header_layer(HeaderName::from_static("cross-origin-opener-policy"), "same-origin"))
        .layer(header_layer(HeaderName::from_static("cross-origin-resource-policy"), "same-origin"))
        .layer(header_layer(HeaderName::from_static("permissions-policy"), PERMISSIONS_POLICY))
        // Logs method and path only: query strings can carry anything.
        .layer(TraceLayer::new_for_http().make_span_with(|request: &Request| {
            tracing::info_span!("request", method = %request.method(), path = %request.uri().path())
        }))
}

fn header_layer(name: HeaderName, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(name, HeaderValue::from_static(value))
}

async fn canonical_host(
    extract::State(state): extract::State<State>,
    request: Request,
    next: Next,
) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    match state.canonical().redirect_for(host, request.uri()) {
        Some(location) => redirect(&location),
        None => next.run(request).await,
    }
}

/// Liveness runs no checks: there is nothing this process depends on that a
/// restart would fix.
async fn live() -> Response {
    no_store(Json(serde_json::json!({ "status": "ok" })))
}

/// Readiness reports what is deployed: the commit and the digest of every
/// embedded file. That is how a rollout is proven from the live origin.
async fn ready(extract::State(state): extract::State<State>) -> Response {
    let site = state.site();
    no_store(Json(serde_json::json!({
        "status": "ok",
        "revision": site::REVISION,
        "site_digest": site.digest(),
        "files": site.len(),
    })))
}

/// A 302, never cached: where the dashboard lives is configuration, and a
/// cached permanent redirect would outlive a change to it.
async fn sign_in(
    extract::State(state): extract::State<State>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    match &state.config.app_url {
        Some(app) => match HeaderValue::try_from(format!("{app}/login")) {
            Ok(location) => no_store((StatusCode::FOUND, [(header::LOCATION, location)])),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        None => serve_site(extract::State(state), method, uri, headers).await,
    }
}

fn no_store(body: impl IntoResponse) -> Response {
    let mut response = body.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn serve_site(
    extract::State(state): extract::State<State>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        let mut response = StatusCode::METHOD_NOT_ALLOWED.into_response();
        response
            .headers_mut()
            .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
        return response;
    }

    let site = state.site();
    match site.resolve(uri.path()) {
        Resolution::Found(entry) => file(entry, StatusCode::OK, &method, &headers),
        Resolution::AddSlash => {
            // Relative to this host. The path passed `normalise`, so it has no
            // empty segment and cannot become `//other.host`.
            let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
            redirect(&format!("{}/{query}", uri.path()))
        }
        Resolution::NotFound => match site.not_found_document() {
            Some(entry) => file(entry, StatusCode::NOT_FOUND, &method, &headers),
            None => StatusCode::NOT_FOUND.into_response(),
        },
    }
}

fn file(
    entry: &'static Entry,
    status: StatusCode,
    method: &Method,
    request: &HeaderMap,
) -> Response {
    let accept = request
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok());
    let (encoding, body) = entry.negotiate(accept);
    let etag = entry.etag(encoding);

    let mut headers = HeaderMap::new();
    if entry.has_variants() {
        headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    }

    // A 404 is not a representation of the requested resource, so it gets
    // neither a validator nor a long-lived cache entry.
    if status != StatusCode::OK {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    } else {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(entry.cache_control()),
        );
        headers.insert(
            header::ETAG,
            HeaderValue::try_from(&etag).expect("hex etag"),
        );
        let not_modified = request
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|candidates| if_none_match_hits(candidates, &etag));
        if not_modified {
            return (StatusCode::NOT_MODIFIED, headers).into_response();
        }
    }

    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(entry.content_type),
    );
    if let Some(coding) = encoding.header_value() {
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static(coding));
    }
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(body.len()));

    // HEAD answers with the headers GET would send, and no body.
    let body = if *method == Method::HEAD {
        Body::empty()
    } else {
        Body::from(body)
    };
    (status, headers, body).into_response()
}

fn redirect(location: &str) -> Response {
    match HeaderValue::try_from(location) {
        Ok(location) => (
            StatusCode::PERMANENT_REDIRECT,
            [
                (header::LOCATION, location),
                (
                    header::CACHE_CONTROL,
                    HeaderValue::from_static(REDIRECT_CACHE),
                ),
            ],
        )
            .into_response(),
        Err(_) => StatusCode::BAD_REQUEST.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::{config::Config, site::tests::fixture_sites};

    fn app(args: &[&str]) -> Router {
        use clap::Parser;
        let mut config =
            Config::try_parse_from(std::iter::once("grund-website").chain(args.iter().copied()))
                .unwrap();
        config.validate().unwrap();
        router(State::new(config, fixture_sites(), None, None))
    }

    async fn send(app: Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        (status, headers, body)
    }

    fn get(path: &str) -> Request<Body> {
        Request::get(path)
            .header("host", "grund.sh")
            .body(Body::empty())
            .unwrap()
    }

    fn header<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }

    #[tokio::test]
    async fn the_home_page_is_html_with_a_validator_and_revalidating_cache() {
        let (status, headers, body) = send(app(&[]), get("/")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(header(&headers, "content-type"), "text/html; charset=utf-8");
        assert_eq!(
            header(&headers, "cache-control"),
            "public, max-age=0, must-revalidate"
        );
        assert_eq!(header(&headers, "etag"), "\"1dex\"");
        assert_eq!(body, b"<h1>home</h1>");
    }

    #[tokio::test]
    async fn a_brotli_capable_client_gets_the_precompressed_body() {
        let request = Request::get("/")
            .header("accept-encoding", "gzip, br")
            .body(Body::empty())
            .unwrap();
        let (_, headers, body) = send(app(&[]), request).await;
        assert_eq!(header(&headers, "content-encoding"), "br");
        assert_eq!(header(&headers, "vary"), "Accept-Encoding");
        assert_eq!(header(&headers, "etag"), "\"1dex-br\"");
        assert_eq!(body, b"BR-HOME");
    }

    #[tokio::test]
    async fn hashed_assets_are_cached_as_immutable() {
        let (status, headers, _) = send(app(&[]), get("/assets/app-3f9a1c7e.css")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            header(&headers, "cache-control"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(header(&headers, "content-type"), "text/css; charset=utf-8");
    }

    #[tokio::test]
    async fn a_matching_etag_answers_304_without_a_body() {
        let request = Request::get("/")
            .header("if-none-match", "\"1dex\"")
            .body(Body::empty())
            .unwrap();
        let (status, headers, body) = send(app(&[]), request).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        assert_eq!(header(&headers, "etag"), "\"1dex\"");
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn head_sends_the_get_headers_and_no_body() {
        let request = Request::head("/").body(Body::empty()).unwrap();
        let (status, headers, body) = send(app(&[]), request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(header(&headers, "content-length"), "13");
        assert_eq!(header(&headers, "content-type"), "text/html; charset=utf-8");
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn sign_in_sends_visitors_to_the_dashboard_login_uncached() {
        let (status, headers, _) = send(
            app(&["--app-url", "https://app.example.com"]),
            get("/sign-in"),
        )
        .await;
        assert_eq!(status, StatusCode::FOUND);
        assert_eq!(
            header(&headers, "location"),
            "https://app.example.com/login"
        );
        assert_eq!(header(&headers, "cache-control"), "no-store");
    }

    #[tokio::test]
    async fn sign_in_is_a_404_where_no_dashboard_is_configured() {
        let (status, _, _) = send(app(&[]), get("/sign-in")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_unknown_path_gets_the_404_document_with_status_404() {
        let (status, headers, body) = send(app(&[]), get("/no/such/page")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(header(&headers, "content-type"), "text/html; charset=utf-8");
        assert_eq!(header(&headers, "cache-control"), "no-cache");
        assert!(headers.get("etag").is_none());
        assert_eq!(body, b"<h1>not here</h1>");
    }

    #[tokio::test]
    async fn a_traversal_attempt_is_an_ordinary_404() {
        let (status, _, body) = send(app(&[]), get("/%2e%2e/%2e%2e/etc/passwd")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, b"<h1>not here</h1>");
    }

    #[tokio::test]
    async fn a_directory_without_its_slash_redirects_keeping_the_query() {
        let (status, headers, _) = send(app(&[]), get("/docs?x=1")).await;
        assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
        assert_eq!(header(&headers, "location"), "/docs/?x=1");
    }

    #[tokio::test]
    async fn an_alias_host_redirects_permanently_to_the_canonical_origin() {
        let request = Request::get("/docs/?ref=x")
            .header("host", "www.grund.sh")
            .body(Body::empty())
            .unwrap();
        let (status, headers, _) = send(app(&["--redirect-hosts", "www.grund.sh"]), request).await;
        assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
        assert_eq!(header(&headers, "location"), "https://grund.sh/docs/?ref=x");
        assert_eq!(header(&headers, "cache-control"), REDIRECT_CACHE);
    }

    #[tokio::test]
    async fn methods_other_than_get_and_head_are_refused_with_allow() {
        let request = Request::post("/").body(Body::empty()).unwrap();
        let (status, headers, _) = send(app(&[]), request).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(header(&headers, "allow"), "GET, HEAD");
    }

    #[tokio::test]
    async fn every_kind_of_response_carries_the_security_headers() {
        let alias = Request::get("/")
            .header("host", "www.grund.sh")
            .body(Body::empty())
            .unwrap();
        let requests = [
            get("/"),
            get("/missing"),
            get("/docs"),
            Request::post("/").body(Body::empty()).unwrap(),
            get("/health/live"),
            alias,
        ];
        for request in requests {
            let path = request.uri().to_string();
            let (_, headers, _) = send(app(&["--redirect-hosts", "www.grund.sh"]), request).await;
            assert_eq!(header(&headers, "content-security-policy"), CSP, "{path}");
            assert_eq!(
                header(&headers, "x-content-type-options"),
                "nosniff",
                "{path}"
            );
            assert_eq!(header(&headers, "x-frame-options"), "DENY", "{path}");
            assert!(headers.get("x-robots-tag").is_none(), "{path}");
            assert!(headers.get("strict-transport-security").is_none(), "{path}");
        }
    }

    #[tokio::test]
    async fn noindex_and_hsts_are_sent_only_when_configured() {
        let (_, headers, _) = send(
            app(&["--noindex", "true", "--hsts-max-age", "300"]),
            get("/"),
        )
        .await;
        assert_eq!(header(&headers, "x-robots-tag"), "noindex, nofollow");
        assert_eq!(header(&headers, "strict-transport-security"), "max-age=300");
    }

    #[tokio::test]
    async fn readiness_reports_the_revision_and_site_digest_and_is_never_cached() {
        let (status, headers, body) = send(app(&[]), get("/health/ready")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(header(&headers, "cache-control"), "no-store");
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["site_digest"], "fixture-digest");
        assert_eq!(body["revision"], site::REVISION);
        assert_eq!(body["files"], 5);
    }
}
