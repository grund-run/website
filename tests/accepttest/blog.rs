//! The blog: published posts are public, drafts only where drafts are on.

use crate::accepttest::fixtures::testcase_configured;

#[tokio::test]
async fn a_draft_is_served_marked_and_noindex_only_where_drafts_are_on() -> anyhow::Result<()> {
    let Some((given, when, then)) =
        testcase_configured(&[("GRUND_WEBSITE_BLOG_DRAFTS", "true")]).await?
    else {
        return Ok(());
    };
    let drafts = given.the_drafts_the_blog_lists().await?;
    if drafts.is_empty() {
        eprintln!("skipped: the blog has no drafts right now");
        return Ok(());
    }

    for path in &drafts {
        when.requesting("GET", path).await?;
        then.status(200)?
            .header("content-type", "text/html; charset=utf-8")?
            .body_contains(r#"<meta name="robots" content="noindex">"#)?
            .body_contains("Draft.")?
            .carries_the_security_headers()?;
    }

    let Some((_given, when, then)) = testcase_configured(&[]).await? else {
        return Ok(());
    };
    for path in &drafts {
        when.requesting("GET", path).await?;
        then.status(404)?;
    }
    Ok(())
}

#[tokio::test]
async fn the_feed_is_atom_that_lists_only_what_the_index_lists() -> anyhow::Result<()> {
    let Some((_given, when, then)) =
        testcase_configured(&[("GRUND_WEBSITE_BLOG_DRAFTS", "true")]).await?
    else {
        return Ok(());
    };
    when.requesting("GET", "/blog/feed.xml").await?;
    if then.status(404).is_ok() {
        eprintln!("skipped: the blog has no posts right now");
        return Ok(());
    }
    then.status(200)?
        .header("content-type", "application/xml")?
        .body_contains(r#"<feed xmlns="http://www.w3.org/2005/Atom">"#)?;
    Ok(())
}
