use anyhow::Context;

use super::{Given, client};

impl Given {
    /// Records the GET response for `path`, to compare a later request with.
    pub async fn the_get_response_of(&self, path: &str) -> anyhow::Result<&Self> {
        let fixture = &self.testcase.fixture;
        let response = client::send(&fixture.origin, "GET", path, None, &[]).await?;
        self.testcase.data().baseline = Some(response);
        Ok(self)
    }

    /// The ETag the recorded baseline carried.
    pub fn its_etag(&self) -> anyhow::Result<String> {
        let data = self.testcase.data();
        let baseline = data.baseline.as_ref().context("record a baseline first")?;
        baseline
            .header("etag")
            .map(str::to_string)
            .context("the baseline has no ETag")
    }

    /// Finds an `/assets/...` file the home page links to. Behaviour, not copy:
    /// whatever the designed site names its assets, it links at least one.
    pub async fn an_asset_the_home_page_links(&self) -> anyhow::Result<&Self> {
        let fixture = &self.testcase.fixture;
        let home = client::send(&fixture.origin, "GET", "/", None, &[]).await?;
        let html = String::from_utf8_lossy(&home.body);
        let start = html
            .find("/assets/")
            .context("the home page links no /assets/ file")?;
        let path: String = html[start..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || "/._~@+-".contains(*c))
            .collect();
        self.testcase.data().asset = Some(path);
        Ok(self)
    }

    /// The alias host this target redirects, if it has one.
    pub fn the_redirect_host(&self) -> Option<String> {
        self.testcase.fixture.expect.redirect_host.clone()
    }

    /// Reads the blog index and records every post it marks as a draft.
    /// Behaviour, not copy: whatever the drafts are called, the index marks
    /// each with `draft-tag` inside its link.
    pub async fn the_drafts_the_blog_lists(&self) -> anyhow::Result<Vec<String>> {
        let fixture = &self.testcase.fixture;
        let response = client::send(&fixture.origin, "GET", "/blog/", None, &[]).await?;
        if response.status != 200 {
            return Ok(Vec::new());
        }
        let body = String::from_utf8_lossy(&response.body).into_owned();
        let drafts: Vec<String> = body
            .split("<li class=\"post-item\">")
            .skip(1)
            .filter(|item| item.contains("draft-tag"))
            .filter_map(|item| {
                let start = item.find("href=\"")? + 6;
                let end = item[start..].find('"')? + start;
                Some(item[start..end].to_string())
            })
            .collect();
        self.testcase.data().drafts = drafts.clone();
        Ok(drafts)
    }
}
