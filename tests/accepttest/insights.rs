//! Page views reported to grund insights: what is sent, what is not, and
//! that pages never depend on it. A stub insights in this process receives
//! the batches.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use crate::accepttest::fixtures::testcase_configured;

/// Accepts connections, keeps each request body, answers 202.
async fn stub_insights() -> anyhow::Result<(String, Arc<Mutex<Vec<String>>>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = bodies.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let seen = seen.clone();
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
                    if raw.len() >= end + 4 + length {
                        seen.lock().unwrap().push(text[end + 4..].to_string());
                        let _ = socket
                            .write_all(b"HTTP/1.1 202 Accepted\r\ncontent-length: 2\r\n\r\n{}")
                            .await;
                        raw.clear();
                    }
                }
            });
        }
    });
    Ok((url, bodies))
}

async fn wait_for(bodies: &Arc<Mutex<Vec<String>>>, needle: &str) -> anyhow::Result<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let all = bodies.lock().unwrap().join("\n");
        if all.contains(needle) {
            return Ok(all);
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "no batch containing {needle:?} within 5 s; got {all:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn a_page_view_reaches_insights_without_its_query_string() -> anyhow::Result<()> {
    let (url, bodies) = stub_insights().await?;
    let Some((_given, when, then)) = testcase_configured(&[
        ("GRUND_WEBSITE_INSIGHTS_URL", &url),
        ("GRUND_WEBSITE_INSIGHTS_CLIENT_IP_HEADER", "X-Real-Ip"),
    ])
    .await?
    else {
        return Ok(());
    };

    when.requesting_with(
        "GET",
        "/pricing?utm_source=hn&utm_campaign=launch&token=secret-in-a-link",
        &[
            ("Referer", "https://news.ycombinator.com/item?id=1"),
            ("X-Real-Ip", "9.9.9.9"),
            ("User-Agent", "curl/8.9.1"),
        ],
    )
    .await?;
    then.status(200)?;
    when.requesting("GET", "/styles.css").await?;
    when.requesting("HEAD", "/").await?;

    let sent = wait_for(&bodies, "/pricing").await?;
    let batch: serde_json::Value = serde_json::from_str(sent.lines().last().unwrap_or("{}"))?;
    let event = &batch["events"][0];
    anyhow::ensure!(event["path"] == "/pricing", "{event}");
    anyhow::ensure!(event["site"] == "grund.sh", "{event}");
    anyhow::ensure!(
        event["utm_source"] == "hn" && event["utm_campaign"] == "launch",
        "{event}"
    );
    anyhow::ensure!(event["referrer_host"] == "news.ycombinator.com", "{event}");
    anyhow::ensure!(event["client_ip"] == "9.9.9.9", "{event}");
    anyhow::ensure!(
        !sent.contains("secret-in-a-link"),
        "the query string left the server: {sent}"
    );
    anyhow::ensure!(
        !sent.contains("styles.css"),
        "an asset was reported as a view: {sent}"
    );
    anyhow::ensure!(
        batch["events"].as_array().map(Vec::len) == Some(1),
        "HEAD or an asset was reported: {sent}"
    );
    Ok(())
}

#[tokio::test]
async fn pages_still_serve_at_once_when_insights_is_down() -> anyhow::Result<()> {
    // A port nothing listens on: every POST fails to connect.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?;
    let url = format!("http://{closed}");
    let Some((_given, when, then)) =
        testcase_configured(&[("GRUND_WEBSITE_INSIGHTS_URL", &url)]).await?
    else {
        return Ok(());
    };

    let started = Instant::now();
    for _ in 0..50 {
        when.requesting("GET", "/").await?;
        then.status(200)?;
    }
    let elapsed = started.elapsed();
    anyhow::ensure!(
        elapsed < Duration::from_secs(5),
        "50 pages took {elapsed:?} with insights down"
    );
    Ok(())
}
