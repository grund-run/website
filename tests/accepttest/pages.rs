use crate::accepttest::fixtures::testcase;

#[tokio::test]
async fn the_home_page_is_html_that_revalidates_against_a_strong_etag() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    when.requesting("GET", "/").await?;

    then.status(200)?
        .header("content-type", "text/html; charset=utf-8")?
        .header("cache-control", "public, max-age=0, must-revalidate")?
        .header_contains("etag", "\"")?
        .header_lacks("etag", "W/")?;
    Ok(())
}

#[tokio::test]
async fn a_request_with_the_current_etag_is_answered_304_without_a_body() -> anyhow::Result<()> {
    let (given, when, then) = testcase().await?;
    let etag = given.the_get_response_of("/").await?.its_etag()?;

    when.requesting_with("GET", "/", &[("If-None-Match", &etag)])
        .await?;

    then.status(304)?
        .header("etag", &etag)?
        .no_body_on_the_wire()?;
    Ok(())
}

#[tokio::test]
async fn head_sends_the_get_headers_and_no_body() -> anyhow::Result<()> {
    let (given, when, then) = testcase().await?;
    given.the_get_response_of("/").await?;

    when.requesting("HEAD", "/").await?;

    then.status(200)?
        .header("content-type", "text/html; charset=utf-8")?
        .content_length_matches_the_get_body()?
        .no_body_on_the_wire()?;
    Ok(())
}

#[tokio::test]
async fn hashed_assets_are_cached_as_immutable() -> anyhow::Result<()> {
    let (given, when, then) = testcase().await?;
    given.an_asset_the_home_page_links().await?;

    when.requesting_the_asset().await?;

    then.status(200)?
        .header("cache-control", "public, max-age=31536000, immutable")?;
    Ok(())
}
