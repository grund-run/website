use super::{When, client};

impl When {
    pub async fn requesting(&self, method: &str, path: &str) -> anyhow::Result<&Self> {
        self.requesting_with(method, path, &[]).await
    }

    pub async fn requesting_with(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> anyhow::Result<&Self> {
        let fixture = &self.testcase.fixture;
        let response = client::send(&fixture.origin, method, path, None, headers).await?;
        self.testcase.data().last = Some(response);
        Ok(self)
    }

    /// GET `path` as `host`. Against https the host is connected to by name
    /// (its own DNS, route and certificate); against http it is only the Host
    /// header on the same address.
    pub async fn requesting_as_host(&self, host: &str, path: &str) -> anyhow::Result<&Self> {
        let fixture = &self.testcase.fixture;
        let response = if fixture.origin.tls {
            client::send(&fixture.origin.with_host(host), "GET", path, None, &[]).await?
        } else {
            client::send(&fixture.origin, "GET", path, Some(host), &[]).await?
        };
        self.testcase.data().last = Some(response);
        Ok(self)
    }

    /// GETs the asset a Given step found.
    pub async fn requesting_the_asset(&self) -> anyhow::Result<&Self> {
        let path = self.testcase.data().asset.clone();
        let path = path.ok_or_else(|| anyhow::anyhow!("find an asset first"))?;
        self.requesting("GET", &path).await
    }
}
