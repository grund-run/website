use crate::accepttest::fixtures::testcase;

#[tokio::test]
async fn an_alias_host_redirects_308_to_the_canonical_origin_keeping_path_and_query()
-> anyhow::Result<()> {
    let (given, when, then) = testcase().await?;
    let Some(alias) = given.the_redirect_host() else {
        eprintln!("skipped: this target has no redirect host (GRUND_WEBSITE_ACCEPT_REDIRECT_HOST)");
        return Ok(());
    };

    when.requesting_as_host(&alias, "/some/path?ref=x").await?;

    then.redirects_to_the_canonical("/some/path?ref=x")?
        .carries_the_security_headers()?;
    Ok(())
}
