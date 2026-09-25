use crate::accepttest::fixtures::{testcase, testcase_configured};

#[tokio::test]
async fn the_home_page_links_sign_in() -> anyhow::Result<()> {
    let (_given, when, then) = testcase().await?;

    when.requesting("GET", "/").await?;

    then.status(200)?.links_to("/sign-in")?;
    Ok(())
}

#[tokio::test]
async fn sign_in_sends_visitors_to_the_dashboard_login_without_caching() -> anyhow::Result<()> {
    let Some((_given, when, then)) =
        testcase_configured(&[("GRUND_WEBSITE_APP_URL", "https://app.example.com")]).await?
    else {
        return Ok(());
    };

    when.requesting("GET", "/sign-in").await?;

    then.status(302)?
        .header("location", "https://app.example.com/login")?
        .header("cache-control", "no-store")?
        .carries_the_security_headers()?;
    Ok(())
}

#[tokio::test]
async fn sign_in_is_not_found_where_no_dashboard_is_configured() -> anyhow::Result<()> {
    let Some((_given, when, then)) = testcase_configured(&[]).await? else {
        return Ok(());
    };

    when.requesting("GET", "/sign-in").await?;

    then.status(404)?.no_header("location")?;
    Ok(())
}
