//! The newsletter sign-up: forms on this origin, relayed to grund insights.
//! A stub insights in this process answers the relay, so these tests run
//! against a spawned binary only. Fake addresses only.

use std::sync::{Arc, Mutex};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use crate::accepttest::fixtures::testcase_configured;

const CONFIRM_TOKEN: &str = "CONFIRM-token-abcdefghijklmnop";
const EXPIRED_TOKEN: &str = "EXPIRED-token-abcdefghijklmnop";
const UNSUB_TOKEN: &str = "UNSUB-token-abcdefghijklmnopqr";

type Seen = Arc<Mutex<Vec<(String, String)>>>;

/// Answers like insights' newsletter routes and keeps (path, body) of each
/// request. A token starting with EXPIRED is unknown (404).
async fn stub_insights() -> anyhow::Result<(String, Seen)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let seen: Seen = Arc::default();
    let record = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let record = record.clone();
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut buf = [0u8; 8192];
                loop {
                    let Ok(n) = socket.read(&mut buf).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&raw).to_string();
                    let Some(end) = text.find("\r\n\r\n") else {
                        continue;
                    };
                    let length = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if raw.len() < end + 4 + length {
                        continue;
                    }
                    let path = text.split(' ').nth(1).unwrap_or("").to_string();
                    let body = text[end + 4..end + 4 + length].to_string();
                    record.lock().unwrap().push((path.clone(), body.clone()));
                    let (status, reply) = match path.as_str() {
                        _ if body.contains("EXPIRED") => ("404 Not Found", "{}".to_string()),
                        "/v1/newsletter/subscriptions" => {
                            ("202 Accepted", r#"{"status":"accepted"}"#.to_string())
                        }
                        "/v1/newsletter/confirm" => (
                            "200 OK",
                            format!(
                                r#"{{"status":"confirmed","unsubscribe_token":"{UNSUB_TOKEN}"}}"#
                            ),
                        ),
                        "/v1/newsletter/unsubscribe" => {
                            ("200 OK", r#"{"status":"unsubscribed"}"#.to_string())
                        }
                        _ => ("404 Not Found", "{}".to_string()),
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{reply}",
                        reply.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    raw.clear();
                }
            });
        }
    });
    Ok((url, seen))
}

fn enabled(url: &str) -> Vec<(&'static str, String)> {
    vec![
        ("GRUND_WEBSITE_NEWSLETTER", "true".into()),
        ("GRUND_WEBSITE_INSIGHTS_URL", url.to_string()),
        (
            "GRUND_WEBSITE_INSIGHTS_CLIENT_IP_HEADER",
            "X-Real-Ip".into(),
        ),
    ]
}

fn env<'a>(pairs: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    pairs.iter().map(|(k, v)| (*k, v.as_str())).collect()
}

const FORM: &str = "email=ada%40example.com&website=&consent=newsletter-2026-10";

#[tokio::test]
async fn a_sign_up_is_relayed_with_its_first_touch_and_lands_on_the_thanks_page()
-> anyhow::Result<()> {
    let (url, seen) = stub_insights().await?;
    let config = enabled(&url);
    let Some((_given, when, then)) = testcase_configured(&env(&config)).await? else {
        return Ok(());
    };

    when.requesting("GET", "/").await?;
    then.status(200)?.body_contains(r#"action="/newsletter""#)?;

    when.posting_form("/newsletter", FORM, &[("X-Real-Ip", "9.9.9.9")])
        .await?;
    then.status(303)?
        .header("location", "/newsletter/thanks")?
        .header("cache-control", "no-store")?;

    let (path, body) = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(path, "/v1/newsletter/subscriptions");
    let body: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(body["email"], "ada@example.com");
    assert_eq!(body["consent"]["given"], true);
    assert_eq!(body["consent"]["text_version"], "newsletter-2026-10");
    assert_eq!(body["client_ip"], "9.9.9.9");
    assert_eq!(body["landing_path"], "/");
    assert_eq!(body["utm_source"], "accept");

    when.requesting("GET", "/newsletter/thanks").await?;
    then.status(200)?.body_contains("Check your inbox")?;
    Ok(())
}

#[tokio::test]
async fn the_mailed_link_shows_a_button_and_only_the_button_confirms() -> anyhow::Result<()> {
    let (url, seen) = stub_insights().await?;
    let config = enabled(&url);
    let Some((_given, when, then)) = testcase_configured(&env(&config)).await? else {
        return Ok(());
    };

    when.requesting("GET", &format!("/newsletter/confirm?token={CONFIRM_TOKEN}"))
        .await?;
    then.status(200)?
        .header("cache-control", "no-store")?
        .body_contains(&format!(r#"value="{CONFIRM_TOKEN}""#))?
        .body_contains("Confirm my subscription")?
        .carries_the_security_headers()?;
    assert!(
        seen.lock().unwrap().is_empty(),
        "opening the link confirms nothing"
    );

    when.posting_form(
        "/newsletter/confirm",
        &format!("token={CONFIRM_TOKEN}"),
        &[],
    )
    .await?;
    then.status(303)?.header(
        "location",
        &format!("/newsletter/confirmed?token={UNSUB_TOKEN}"),
    )?;

    when.requesting("GET", &format!("/newsletter/confirmed?token={UNSUB_TOKEN}"))
        .await?;
    then.status(200)?
        .body_contains(&format!("/newsletter/unsubscribe?token={UNSUB_TOKEN}"))?;

    when.requesting(
        "GET",
        &format!("/newsletter/unsubscribe?token={UNSUB_TOKEN}"),
    )
    .await?;
    then.status(200)?
        .body_contains(&format!(r#"value="{UNSUB_TOKEN}""#))?;
    when.posting_form(
        "/newsletter/unsubscribe",
        &format!("token={UNSUB_TOKEN}"),
        &[],
    )
    .await?;
    then.status(303)?
        .header("location", "/newsletter/unsubscribed")?;
    Ok(())
}

#[tokio::test]
async fn an_expired_or_malformed_link_lands_on_the_expired_page() -> anyhow::Result<()> {
    let (url, _seen) = stub_insights().await?;
    let config = enabled(&url);
    let Some((_given, when, then)) = testcase_configured(&env(&config)).await? else {
        return Ok(());
    };

    when.posting_form(
        "/newsletter/confirm",
        &format!("token={EXPIRED_TOKEN}"),
        &[],
    )
    .await?;
    then.status(303)?
        .header("location", "/newsletter/expired")?;
    when.requesting("GET", "/newsletter/confirm?token=%22%3E%3Cscript%3E")
        .await?;
    then.status(303)?
        .header("location", "/newsletter/expired")?;
    Ok(())
}

#[tokio::test]
async fn with_insights_down_the_visitor_gets_the_error_page_never_a_500() -> anyhow::Result<()> {
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.local_addr()?.port()
    };
    let config = enabled(&format!("http://127.0.0.1:{closed}"));
    let Some((_given, when, then)) = testcase_configured(&env(&config)).await? else {
        return Ok(());
    };

    when.posting_form("/newsletter", FORM, &[]).await?;
    then.status(303)?.header("location", "/newsletter/error")?;
    when.requesting("GET", "/newsletter/error").await?;
    then.status(200)?.body_contains("That did not work")?;
    Ok(())
}

#[tokio::test]
async fn a_form_posted_from_another_site_is_refused_and_a_bot_is_left_to_insights()
-> anyhow::Result<()> {
    let (url, seen) = stub_insights().await?;
    let config = enabled(&url);
    let Some((_given, when, then)) = testcase_configured(&env(&config)).await? else {
        return Ok(());
    };

    when.posting_form("/newsletter", FORM, &[("Origin", "https://evil.example")])
        .await?;
    then.status(403)?;
    assert!(seen.lock().unwrap().is_empty());

    when.posting_form(
        "/newsletter",
        "email=bot%40example.com&website=https%3A%2F%2Fspam.example&consent=newsletter-2026-10",
        &[],
    )
    .await?;
    then.status(303)?.header("location", "/newsletter/thanks")?;
    let (_, body) = seen.lock().unwrap().last().cloned().unwrap();
    assert!(
        body.contains("spam.example"),
        "the honeypot reaches insights, which discards it"
    );
    Ok(())
}

#[tokio::test]
async fn with_the_newsletter_off_there_is_no_form_and_no_route() -> anyhow::Result<()> {
    let Some((_given, when, then)) = testcase_configured(&[]).await? else {
        return Ok(());
    };
    when.requesting("GET", "/").await?;
    then.status(200)?.body_lacks(r#"action="/newsletter""#)?;
    when.posting_form("/newsletter", FORM, &[]).await?;
    then.status(404)?;
    Ok(())
}
