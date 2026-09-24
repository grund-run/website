use crate::accepttest::fixtures::testcase;

#[tokio::test]
async fn an_unknown_path_gets_the_404_document_and_is_not_cached_as_a_page() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    when.requesting("GET", "/definitely/not/a/page").await?;

    then.status(404)?
        .header_contains("content-type", "text/html")?
        .header("cache-control", "no-cache")?
        .no_header("etag")?;
    Ok(())
}

/// Sent byte for byte, unnormalised: nothing outside the embedded site is
/// reachable however the path is spelled.
#[tokio::test]
async fn traversal_attempts_are_ordinary_404s() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    for probe in [
        "/../Cargo.toml",
        "/%2e%2e/%2e%2e/etc/passwd",
        "/assets/..%2f..%2fCargo.toml",
        "/assets/..%5c..%5cCargo.toml",
        "/.git/config",
        "//etc/passwd",
    ] {
        when.requesting("GET", probe).await?;
        then.status(404).map_err(|error| error.context(probe))?;
    }
    Ok(())
}

#[tokio::test]
async fn methods_other_than_get_and_head_are_refused_with_allow() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    when.requesting("POST", "/").await?;

    then.status(405)?.header("allow", "GET, HEAD")?;
    Ok(())
}
