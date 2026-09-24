use crate::accepttest::fixtures::testcase;

#[tokio::test]
async fn liveness_answers_ok_as_json_and_is_never_cached() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    when.requesting("GET", "/health/live").await?;

    then.status(200)?
        .header_contains("content-type", "application/json")?
        .header("cache-control", "no-store")?
        .json_field_is_set("status")?;
    Ok(())
}

/// How a deployment is proven from the live origin: the commit and site digest
/// the running binary was built from.
#[tokio::test]
async fn readiness_reports_what_is_deployed() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    when.requesting("GET", "/health/ready").await?;

    then.status(200)?
        .header("cache-control", "no-store")?
        .json_field_is_set("revision")?
        .json_field_is_set("site_digest")?;
    Ok(())
}
