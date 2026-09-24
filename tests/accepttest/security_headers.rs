use crate::accepttest::fixtures::{testcase, testcase_configured};

#[tokio::test]
async fn pages_404s_and_refusals_all_carry_the_security_headers() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    for (method, path) in [
        ("GET", "/"),
        ("GET", "/definitely/not/a/page"),
        ("POST", "/"),
    ] {
        when.requesting(method, path).await?;
        then.carries_the_security_headers()
            .map_err(|error| error.context(format!("{method} {path}")))?;
    }
    Ok(())
}

#[tokio::test]
async fn the_target_asks_to_be_indexed_or_not_as_configured() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    when.requesting("GET", "/").await?;

    then.follows_the_indexing_policy()?;
    Ok(())
}

#[tokio::test]
async fn a_noindex_deployment_says_so_on_every_response() -> anyhow::Result<()> {
    let Some((_given, when, then)) =
        testcase_configured(&[("GRUND_WEBSITE_NOINDEX", "true")]).await?
    else {
        return Ok(());
    };

    for path in ["/", "/definitely/not/a/page"] {
        when.requesting("GET", path).await?;
        then.header("x-robots-tag", "noindex, nofollow")?;
    }
    Ok(())
}
