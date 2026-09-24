use crate::accepttest::fixtures::testcase;

#[tokio::test]
async fn a_client_accepting_brotli_gets_the_precompressed_brotli_body() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    when.requesting_with("GET", "/", &[("Accept-Encoding", "br, gzip")])
        .await?;

    then.status(200)?
        .header("content-encoding", "br")?
        .header("vary", "Accept-Encoding")?;
    Ok(())
}

#[tokio::test]
async fn a_gzip_only_client_gets_gzip() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    when.requesting_with("GET", "/", &[("Accept-Encoding", "gzip")])
        .await?;

    then.status(200)?.header("content-encoding", "gzip")?;
    Ok(())
}

#[tokio::test]
async fn a_client_that_accepts_no_encoding_gets_identity() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    when.requesting("GET", "/").await?;

    then.status(200)?.no_header("content-encoding")?;
    Ok(())
}
