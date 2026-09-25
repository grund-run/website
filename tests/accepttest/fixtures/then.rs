use anyhow::{Context, ensure};

use super::{Then, client::Response};

impl Then {
    fn last(&self) -> anyhow::Result<Response> {
        self.testcase
            .data()
            .last
            .clone()
            .context("no request was made")
    }

    pub fn status(&self, expected: u16) -> anyhow::Result<&Self> {
        let status = self.last()?.status;
        ensure!(
            status == expected,
            "status: got {status}, wanted {expected}"
        );
        Ok(self)
    }

    pub fn status_in(&self, allowed: &[u16]) -> anyhow::Result<&Self> {
        let status = self.last()?.status;
        ensure!(
            allowed.contains(&status),
            "status: got {status}, wanted one of {allowed:?}"
        );
        Ok(self)
    }

    pub fn header(&self, name: &str, expected: &str) -> anyhow::Result<&Self> {
        let response = self.last()?;
        let value = response.header(name);
        ensure!(
            value == Some(expected),
            "{name}: got {value:?}, wanted {expected:?}"
        );
        Ok(self)
    }

    pub fn header_contains(&self, name: &str, needle: &str) -> anyhow::Result<&Self> {
        let response = self.last()?;
        let value = response.header(name).unwrap_or_default();
        ensure!(value.contains(needle), "{name}: {value:?} lacks {needle:?}");
        Ok(self)
    }

    pub fn header_lacks(&self, name: &str, needle: &str) -> anyhow::Result<&Self> {
        let response = self.last()?;
        let value = response.header(name).unwrap_or_default();
        ensure!(
            !value.contains(needle),
            "{name}: {value:?} contains {needle:?}"
        );
        Ok(self)
    }

    pub fn no_header(&self, name: &str) -> anyhow::Result<&Self> {
        let response = self.last()?;
        ensure!(
            response.header(name).is_none(),
            "{name} is present: {:?}",
            response.header(name)
        );
        Ok(self)
    }

    /// No byte after the headers: read to EOF on a closed connection, so
    /// nothing the server sent is hidden.
    /// The body links `path` with `href="{path}"`.
    pub fn links_to(&self, path: &str) -> anyhow::Result<&Self> {
        let body = self.last()?.body;
        let needle = format!("href=\"{path}\"");
        ensure!(
            String::from_utf8_lossy(&body).contains(&needle),
            "the page has no link to {path}"
        );
        Ok(self)
    }

    pub fn no_body_on_the_wire(&self) -> anyhow::Result<&Self> {
        let length = self.last()?.body.len();
        ensure!(length == 0, "{length} body bytes were sent");
        Ok(self)
    }

    /// The Content-Length equals the size of the body the baseline GET received.
    pub fn content_length_matches_the_get_body(&self) -> anyhow::Result<&Self> {
        let get_length = self
            .testcase
            .data()
            .baseline
            .as_ref()
            .context("record a baseline first")?
            .body
            .len();
        let response = self.last()?;
        let declared = response
            .header("content-length")
            .unwrap_or_default()
            .to_string();
        ensure!(
            declared == get_length.to_string(),
            "content-length {declared:?}, GET body {get_length}"
        );
        Ok(self)
    }

    /// A string field of the JSON body is present and non-empty.
    pub fn json_field_is_set(&self, field: &str) -> anyhow::Result<&Self> {
        let json: serde_json::Value =
            serde_json::from_slice(&self.last()?.body).context("body is not JSON")?;
        let value = json[field].as_str().unwrap_or_default();
        ensure!(!value.is_empty(), "{field} is missing or empty in {json}");
        Ok(self)
    }

    /// The security headers every response carries, whatever its status.
    pub fn carries_the_security_headers(&self) -> anyhow::Result<&Self> {
        self.header_contains("content-security-policy", "default-src 'none'")?
            .header_contains("content-security-policy", "script-src 'self'")?
            .header_contains("content-security-policy", "frame-ancestors 'none'")?
            .header_contains("content-security-policy", "base-uri 'none'")?
            .header_lacks("content-security-policy", "unsafe-inline")?
            .header_lacks("content-security-policy", "unsafe-eval")?
            .header("x-content-type-options", "nosniff")?
            .header("referrer-policy", "strict-origin-when-cross-origin")?
            .header("x-frame-options", "DENY")
    }

    /// Whether the response asks not to be indexed matches the target's config.
    pub fn follows_the_indexing_policy(&self) -> anyhow::Result<&Self> {
        if self.testcase.fixture.expect.noindex {
            self.header_contains("x-robots-tag", "noindex")
        } else {
            self.no_header("x-robots-tag")
        }
    }

    /// A 308 to `path_and_query` on the target's canonical origin.
    pub fn redirects_to_the_canonical(&self, path_and_query: &str) -> anyhow::Result<&Self> {
        let canonical = self.testcase.fixture.expect.canonical_origin.clone();
        self.status(308)?
            .header("location", &format!("{canonical}{path_and_query}"))
    }
}
