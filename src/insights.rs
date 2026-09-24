//! Page views for grund insights, reported from the server. Off unless
//! `GRUND_WEBSITE_INSIGHTS_URL` is set.
//!
//! The server already sees every request, so it reports what it served: the
//! path without its query string, the status, the referrer's host, three utm
//! tags, the user agent and the client address from a configured header.
//! insights keeps the first four and uses the last two only to count daily
//! visitors, then drops them (grund/insights docs/design/tracking.md). No
//! script runs in the browser and nothing is stored on the visitor's device.
//!
//! The request path never waits for this. The middleware builds one small
//! struct after the response exists and `try_send`s it into a bounded queue.
//! A full queue drops the event. A background component batches the queue and
//! POSTs it with a short timeout; a failed POST drops the batch. insights being
//! down costs page views in insights, never a page here.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{self, Request},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::Response,
};
use http_body_util::Full;
use hyper::body::Bytes;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use notmad::{Component, ComponentInfo, MadError};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::state::State;

/// Events waiting to be sent. Enough for a burst of ~10 s at 100 req/s; past
/// that, dropping is the point.
pub const QUEUE: usize = 1024;
const BATCH: usize = 100;
const FLUSH_EVERY: Duration = Duration::from_secs(1);
const POST_TIMEOUT: Duration = Duration::from_secs(2);

/// One page view, as sent to insights (its `POST /v1/pageviews` contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageView {
    pub occurred_at_ms: u64,
    pub path: String,
    pub status: u16,
    pub referrer_host: Option<String>,
    pub utm_source: Option<String>,
    pub utm_medium: Option<String>,
    pub utm_campaign: Option<String>,
    pub user_agent: Option<String>,
    pub client_ip: Option<String>,
}

/// File types that are parts of a page, not pages. Their requests are not
/// views.
const ASSET_EXTENSIONS: &[&str] = &[
    ".css",
    ".js",
    ".mjs",
    ".map",
    ".svg",
    ".png",
    ".jpg",
    ".jpeg",
    ".gif",
    ".webp",
    ".avif",
    ".ico",
    ".woff",
    ".woff2",
    ".json",
    ".xml",
    ".webmanifest",
];

/// Whether a request is a page view: a GET for a document that was served
/// (2xx), revalidated (304) or missing (404). HEAD, redirects, assets and
/// health probes are not.
pub fn is_view(method: &Method, path: &str, status: StatusCode) -> bool {
    let status = status.as_u16();
    method == Method::GET
        && ((200..300).contains(&status) || status == 304 || status == 404)
        && !path.starts_with("/assets/")
        && !path.starts_with("/health/")
        && !ASSET_EXTENSIONS
            .iter()
            .any(|ext| path.to_ascii_lowercase().ends_with(ext))
}

/// Builds the event from what the request carried. Pure, so the rules are
/// unit-tested: only the path (no query), only three utm tags, only the
/// referrer's host.
pub fn page_view(
    path: &str,
    query: Option<&str>,
    status: StatusCode,
    headers: &HeaderMap,
    client_ip_header: Option<&str>,
    now: SystemTime,
) -> PageView {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let (mut source, mut medium, mut campaign) = (None, None, None);
    for pair in query.unwrap_or_default().split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let slot = match key {
            "utm_source" => &mut source,
            "utm_medium" => &mut medium,
            "utm_campaign" => &mut campaign,
            _ => continue,
        };
        let decoded = percent_encoding::percent_decode_str(&value.replace('+', " "))
            .decode_utf8_lossy()
            .trim()
            .chars()
            .take(100)
            .collect::<String>();
        if slot.is_none() && !decoded.is_empty() {
            *slot = Some(decoded);
        }
    }
    PageView {
        occurred_at_ms: now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        path: path.chars().take(512).collect(),
        status: status.as_u16(),
        referrer_host: header("referer").and_then(referrer_host),
        utm_source: source,
        utm_medium: medium,
        utm_campaign: campaign,
        user_agent: header("user-agent").map(|ua| ua.chars().take(512).collect()),
        client_ip: client_ip_header
            .and_then(header)
            .map(|v| v.split(',').next().unwrap_or_default().trim().to_string())
            .filter(|v| !v.is_empty() && v.len() <= 64),
    }
}

/// `https://news.ycombinator.com/item?id=1` → `news.ycombinator.com`.
fn referrer_host(referer: &str) -> Option<String> {
    let rest = referer
        .strip_prefix("https://")
        .or_else(|| referer.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.split(':').next()?.to_ascii_lowercase();
    (!host.is_empty() && host.len() <= 253).then_some(host)
}

fn json(site: &str, events: &[PageView]) -> Vec<u8> {
    let events: Vec<_> = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "site": site,
                "occurred_at_ms": e.occurred_at_ms,
                "path": e.path,
                "status": e.status,
                "referrer_host": e.referrer_host,
                "utm_source": e.utm_source,
                "utm_medium": e.utm_medium,
                "utm_campaign": e.utm_campaign,
                "user_agent": e.user_agent,
                "client_ip": e.client_ip,
            })
        })
        .collect();
    serde_json::to_vec(&serde_json::json!({ "events": events })).unwrap_or_default()
}

/// The request side: a queue and what to read from each request.
#[derive(Clone)]
pub struct Insights {
    queue: mpsc::Sender<PageView>,
    client_ip_header: Option<Arc<str>>,
    dropped: Arc<AtomicU64>,
}

impl Insights {
    pub fn channel(client_ip_header: Option<String>) -> (Self, mpsc::Receiver<PageView>) {
        let (queue, receiver) = mpsc::channel(QUEUE);
        (
            Self {
                queue,
                client_ip_header: client_ip_header.map(Arc::from),
                dropped: Arc::default(),
            },
            receiver,
        )
    }

    fn offer(&self, view: PageView) {
        if self.queue.try_send(view).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Middleware: serve the request, then report it if it was a page view.
pub async fn capture(
    extract::State(state): extract::State<State>,
    request: Request,
    next: Next,
) -> Response {
    let Some(insights) = state.insights.clone() else {
        return next.run(request).await;
    };
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let query = request.uri().query().map(str::to_string);
    let headers = request.headers().clone();
    let response = next.run(request).await;
    if is_view(&method, &path, response.status()) {
        insights.offer(page_view(
            &path,
            query.as_deref(),
            response.status(),
            &headers,
            insights.client_ip_header.as_deref(),
            SystemTime::now(),
        ));
    }
    response
}

type HttpClient = Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>;

/// The sending side, as a notmad component.
pub struct Sender {
    endpoint: hyper::Uri,
    site: String,
    token: Option<String>,
    receiver: tokio::sync::Mutex<mpsc::Receiver<PageView>>,
    dropped: Arc<AtomicU64>,
}

impl Sender {
    pub fn new(
        base_url: &str,
        site: String,
        token: Option<String>,
        insights: &Insights,
        receiver: mpsc::Receiver<PageView>,
    ) -> anyhow::Result<Self> {
        let endpoint = format!("{}/v1/pageviews", base_url.trim_end_matches('/')).parse()?;
        Ok(Self {
            endpoint,
            site,
            token,
            receiver: tokio::sync::Mutex::new(receiver),
            dropped: insights.dropped.clone(),
        })
    }

    async fn post(&self, client: &HttpClient, batch: &[PageView]) {
        let mut request = hyper::Request::post(self.endpoint.clone())
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::USER_AGENT, "grund-website");
        if let Some(token) = &self.token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let Ok(request) = request.body(Full::new(Bytes::from(json(&self.site, batch)))) else {
            return;
        };
        let outcome = tokio::time::timeout(POST_TIMEOUT, client.request(request)).await;
        match outcome {
            Ok(Ok(response)) if response.status() == StatusCode::ACCEPTED => {}
            Ok(Ok(response)) => {
                self.dropped
                    .fetch_add(batch.len() as u64, Ordering::Relaxed);
                tracing::debug!(status = %response.status(), "insights refused a batch; dropped");
            }
            Ok(Err(_)) | Err(_) => {
                self.dropped
                    .fetch_add(batch.len() as u64, Ordering::Relaxed);
            }
        }
    }
}

impl Component for Sender {
    fn info(&self) -> ComponentInfo {
        "grund-website/insights".into()
    }

    async fn run(&self, cancellation: CancellationToken) -> Result<(), MadError> {
        let client: HttpClient = Client::builder(TokioExecutor::new()).build_http();
        let mut receiver = self.receiver.lock().await;
        let mut batch = Vec::with_capacity(BATCH);
        let mut flush = tokio::time::interval(FLUSH_EVERY);
        flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut report = tokio::time::interval(Duration::from_secs(60));
        report.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                () = cancellation.cancelled() => {
                    // One last, bounded send of what is already queued.
                    while let Ok(view) = receiver.try_recv() {
                        batch.push(view);
                    }
                    for chunk in batch.chunks(BATCH) {
                        self.post(&client, chunk).await;
                    }
                    return Ok(());
                }
                view = receiver.recv() => {
                    match view {
                        Some(view) => {
                            batch.push(view);
                            if batch.len() >= BATCH {
                                self.post(&client, &batch).await;
                                batch.clear();
                            }
                        }
                        None => return Ok(()),
                    }
                }
                _ = flush.tick() => {
                    if !batch.is_empty() {
                        self.post(&client, &batch).await;
                        batch.clear();
                    }
                }
                _ = report.tick() => {
                    let dropped = self.dropped.swap(0, Ordering::Relaxed);
                    if dropped > 0 {
                        tracing::warn!(dropped, "page views not delivered to insights in the last minute; pages were unaffected");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    #[test]
    fn documents_are_views_and_their_parts_are_not() {
        let ok = StatusCode::OK;
        assert!(is_view(&Method::GET, "/", ok));
        assert!(is_view(&Method::GET, "/pricing", ok));
        assert!(is_view(&Method::GET, "/install", ok));
        assert!(is_view(&Method::GET, "/licenses/inter.txt", ok));
        assert!(is_view(&Method::GET, "/missing", StatusCode::NOT_FOUND));
        assert!(is_view(&Method::GET, "/", StatusCode::NOT_MODIFIED));
        assert!(!is_view(&Method::GET, "/styles.css", ok));
        assert!(!is_view(&Method::GET, "/favicon.svg", ok));
        assert!(!is_view(
            &Method::GET,
            "/assets/inter-latin-3100e775.woff2",
            ok
        ));
        assert!(!is_view(&Method::GET, "/health/ready", ok));
        assert!(!is_view(&Method::HEAD, "/", ok));
        assert!(!is_view(&Method::POST, "/", StatusCode::METHOD_NOT_ALLOWED));
        assert!(!is_view(
            &Method::GET,
            "/docs",
            StatusCode::PERMANENT_REDIRECT
        ));
    }

    #[test]
    fn only_three_utm_tags_leave_the_query_string() {
        let view = page_view(
            "/pricing",
            Some(
                "utm_source=news%20letter&token=secret&utm_campaign=launch+week&utm_medium=&email=a%40example.com",
            ),
            StatusCode::OK,
            &HeaderMap::new(),
            None,
            UNIX_EPOCH,
        );
        assert_eq!(view.utm_source.as_deref(), Some("news letter"));
        assert_eq!(view.utm_campaign.as_deref(), Some("launch week"));
        assert_eq!(view.utm_medium, None);
        let sent = String::from_utf8(json("grund.sh", &[view])).unwrap();
        assert!(
            !sent.contains("secret") && !sent.contains("example.com"),
            "{sent}"
        );
    }

    #[test]
    fn only_the_referrers_host_is_kept() {
        let h = headers(&[(
            "referer",
            "https://user:pw@News.Ycombinator.com:443/item?id=42#top",
        )]);
        let view = page_view("/", None, StatusCode::OK, &h, None, UNIX_EPOCH);
        assert_eq!(view.referrer_host.as_deref(), Some("news.ycombinator.com"));
        let h = headers(&[("referer", "android-app://com.example")]);
        assert_eq!(
            page_view("/", None, StatusCode::OK, &h, None, UNIX_EPOCH).referrer_host,
            None
        );
    }

    #[test]
    fn the_client_address_comes_only_from_the_configured_header() {
        let h = headers(&[
            ("x-real-ip", "9.9.9.9"),
            ("x-forwarded-for", "1.1.1.1, 10.0.0.1"),
        ]);
        assert_eq!(
            page_view("/", None, StatusCode::OK, &h, None, UNIX_EPOCH).client_ip,
            None
        );
        assert_eq!(
            page_view("/", None, StatusCode::OK, &h, Some("x-real-ip"), UNIX_EPOCH)
                .client_ip
                .as_deref(),
            Some("9.9.9.9")
        );
        assert_eq!(
            page_view(
                "/",
                None,
                StatusCode::OK,
                &h,
                Some("x-forwarded-for"),
                UNIX_EPOCH
            )
            .client_ip
            .as_deref(),
            Some("1.1.1.1")
        );
    }

    #[test]
    fn a_full_queue_drops_instead_of_waiting() {
        let (insights, _receiver) = Insights::channel(None);
        let view = page_view(
            "/",
            None,
            StatusCode::OK,
            &HeaderMap::new(),
            None,
            UNIX_EPOCH,
        );
        for _ in 0..QUEUE + 10 {
            insights.offer(view.clone());
        }
        assert_eq!(insights.dropped.load(Ordering::Relaxed), 10);
    }
}
