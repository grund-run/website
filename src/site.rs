//! The embedded site: a sorted, immutable table of files built by `build.rs`.
//!
//! Everything here is pure. Resolving a request path, choosing an encoding and
//! deciding a cache policy do no I/O, so they are tested directly and the HTTP
//! layer in `api.rs` only turns the answers into responses.
//!
//! Path traversal is impossible by construction rather than by filtering: a
//! path can only ever name an entry in the table, and the table holds exactly
//! the files that were under `site/` at build time. The segment checks below
//! exist so that odd spellings (`..`, `%2e%2e`, `a//b`, `\`) are a plain 404
//! instead of depending on how a lookup happens to normalise them.

use percent_encoding::percent_decode_str;

use crate::state::State;

/// One file of the site, with its precompressed variants.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    /// URL path without the leading slash, e.g. `assets/app-3f9a1c.css`.
    pub path: &'static str,
    pub content_type: &'static str,
    /// First 128 bits of the SHA-256 of the identity bytes, hex.
    pub hash: &'static str,
    pub identity: &'static [u8],
    pub br: Option<&'static [u8]>,
    pub gzip: Option<&'static [u8]>,
}

/// The site as it stands from `from` (Unix seconds) until the next variant.
/// Scheduled blog posts are why there is more than one.
#[derive(Debug)]
pub struct Variant {
    pub from: i64,
    pub entries: &'static [Entry],
    pub digest: &'static str,
}

mod embedded {
    use super::{Entry, Variant};
    include!(concat!(env!("OUT_DIR"), "/site_entries.rs"));
}

/// The build that produced this binary: the commit, and a digest of every
/// embedded path and hash. Readiness reports both, which is how a deployment is
/// proven from the live origin rather than inferred from a tag.
pub const REVISION: &str = embedded::REVISION;

/// Files under this prefix must have content-hashed names, and are served as
/// immutable for a year. Everything else revalidates on every use.
pub const IMMUTABLE_PREFIX: &str = "assets/";

#[derive(Clone, Copy)]
pub struct Site {
    entries: &'static [Entry],
    digest: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Resolution {
    Found(&'static Entry),
    /// `/docs` when only `docs/index.html` exists: redirect to `/docs/` so
    /// relative links in that document resolve against the right base.
    AddSlash,
    NotFound,
}

impl Site {
    /// The site compiled into this binary as it stands now. With `drafts`,
    /// blog drafts and scheduled posts are served too
    /// (GRUND_WEBSITE_BLOG_DRAFTS, dev); without, only what is published.
    #[cfg(test)]
    pub fn embedded(drafts: bool) -> Self {
        Sites::embedded(drafts, false).at(now())
    }

    /// `entries` must be sorted by path; `build.rs` guarantees it for the
    /// embedded table.
    pub const fn new(entries: &'static [Entry], digest: &'static str) -> Self {
        Self { entries, digest }
    }

    pub fn digest(&self) -> &'static str {
        self.digest
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn entries(&self) -> &'static [Entry] {
        self.entries
    }

    pub fn get(&self, path: &str) -> Option<&'static Entry> {
        self.entries
            .binary_search_by(|entry| entry.path.cmp(path))
            .ok()
            .map(|index| &self.entries[index])
    }

    /// The document served, with status 404, for any path that resolves to
    /// nothing. `build.rs` refuses to build a site without one.
    pub fn not_found_document(&self) -> Option<&'static Entry> {
        self.get("404.html")
    }

    /// Maps a raw (still percent-encoded) request path to an entry.
    ///
    /// `/` and `/dir/` serve `index.html` / `dir/index.html`; `/page` serves
    /// `page`, `page.html` or `page.sh`; `/dir` redirects to `/dir/` when only
    /// `dir/index.html` exists.
    pub fn resolve(&self, raw_path: &str) -> Resolution {
        let Some(key) = normalise(raw_path) else {
            return Resolution::NotFound;
        };
        if key.is_empty() {
            return self.found(self.get("index.html"));
        }
        if let Some(dir) = key.strip_suffix('/') {
            return self.found(self.get(&format!("{dir}/index.html")));
        }
        if let Some(entry) = self.get(&key) {
            return Resolution::Found(entry);
        }
        if let Some(entry) = self.get(&format!("{key}.html")) {
            return Resolution::Found(entry);
        }
        // `/install` serves `install.sh`, so the one-line install stays short:
        // `curl -fsSL grund.sh/install | sh`.
        if let Some(entry) = self.get(&format!("{key}.sh")) {
            return Resolution::Found(entry);
        }
        if self.get(&format!("{key}/index.html")).is_some() {
            return Resolution::AddSlash;
        }
        Resolution::NotFound
    }

    fn found(&self, entry: Option<&'static Entry>) -> Resolution {
        entry.map_or(Resolution::NotFound, Resolution::Found)
    }
}

/// Decodes the path and checks every segment. Returns the path without its
/// leading slash (keeping a trailing one), or `None` for anything that is not
/// a plain path to a file.
fn normalise(raw_path: &str) -> Option<String> {
    let decoded = percent_decode_str(raw_path).decode_utf8().ok()?;
    let rest = decoded.strip_prefix('/')?;
    if rest.is_empty() {
        return Some(String::new());
    }
    let body = rest.strip_suffix('/').unwrap_or(rest);
    let plain = body.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && !segment.contains(['\\', '\0'])
            && !segment.chars().any(char::is_control)
    });
    plain.then(|| rest.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Brotli,
    Gzip,
    Identity,
}

impl Encoding {
    pub fn header_value(self) -> Option<&'static str> {
        match self {
            Encoding::Brotli => Some("br"),
            Encoding::Gzip => Some("gzip"),
            Encoding::Identity => None,
        }
    }

    fn etag_suffix(self) -> &'static str {
        match self {
            Encoding::Brotli => "-br",
            Encoding::Gzip => "-gz",
            Encoding::Identity => "",
        }
    }
}

impl Entry {
    /// Picks the representation for an `Accept-Encoding` header value.
    ///
    /// The highest q-value wins; ties prefer brotli, then gzip, then identity.
    /// A coding with `q=0` is never chosen. Identity is always available (we do
    /// not answer 406), which RFC 9110 §12.5.3 permits.
    pub fn negotiate(&self, accept_encoding: Option<&str>) -> (Encoding, &'static [u8]) {
        let accept = accept_encoding.unwrap_or("");
        let mut best = (Encoding::Identity, self.identity, 0.0_f32);
        for (encoding, body) in [(Encoding::Gzip, self.gzip), (Encoding::Brotli, self.br)] {
            let Some(body) = body else { continue };
            let q = quality(accept, encoding.header_value().unwrap());
            if q > 0.0 && q >= best.2 {
                best = (encoding, body, q);
            }
        }
        (best.0, best.1)
    }

    pub fn has_variants(&self) -> bool {
        self.br.is_some() || self.gzip.is_some()
    }

    /// A strong validator per representation, so a cached brotli body is never
    /// revalidated as if it were the gzip one.
    pub fn etag(&self, encoding: Encoding) -> String {
        format!("\"{}{}\"", self.hash, encoding.etag_suffix())
    }

    pub fn cache_control(&self) -> &'static str {
        if self.path.starts_with(IMMUTABLE_PREFIX) {
            "public, max-age=31536000, immutable"
        } else {
            // Stored, but checked against the ETag on every use: a new deploy is
            // visible on the next page load, and an unchanged page costs a 304.
            "public, max-age=0, must-revalidate"
        }
    }
}

/// The q-value the client gave `coding`, falling back to `*`, else 0.
fn quality(accept: &str, coding: &str) -> f32 {
    let mut wildcard = None;
    for item in accept.split(',') {
        let mut parts = item.split(';');
        let name = parts.next().unwrap_or("").trim();
        let q = parts
            .find_map(|param| {
                let (key, value) = param.split_once('=')?;
                key.trim()
                    .eq_ignore_ascii_case("q")
                    .then(|| value.trim().parse::<f32>().ok())
                    .flatten()
            })
            .unwrap_or(1.0);
        if name.eq_ignore_ascii_case(coding) {
            return q;
        }
        if name == "*" {
            wildcard = Some(q);
        }
    }
    wildcard.unwrap_or(0.0)
}

/// Weak comparison, as RFC 9110 §13.1.2 requires for `If-None-Match`.
pub fn if_none_match_hits(header: &str, etag: &str) -> bool {
    let opaque = |tag: &str| tag.trim().trim_start_matches("W/").to_string();
    let ours = opaque(etag);
    header
        .split(',')
        .any(|candidate| candidate.trim() == "*" || opaque(candidate) == ours)
}

/// Every variant of the site in this binary, and the rule for which one is
/// served: the last whose `from` has passed. A scheduled post therefore
/// appears on the first request after its moment, with no rebuild.
#[derive(Clone, Copy)]
pub struct Sites {
    variants: &'static [Variant],
}

impl Sites {
    /// With `newsletter`, pages keep their `<!-- newsletter -->` blocks (the
    /// sign-up form, GRUND_WEBSITE_NEWSLETTER); without, the build removed them.
    pub fn embedded(drafts: bool, newsletter: bool) -> Self {
        let variants = match (drafts, newsletter) {
            (false, false) => embedded::PUBLIC_VARIANTS,
            (true, false) => embedded::DRAFT_VARIANTS,
            (false, true) => embedded::PUBLIC_VARIANTS_NEWSLETTER,
            (true, true) => embedded::DRAFT_VARIANTS_NEWSLETTER,
        };
        Self::new(variants)
    }

    /// `variants` must be sorted by `from`, the first from `i64::MIN`;
    /// `build.rs` guarantees it.
    pub const fn new(variants: &'static [Variant]) -> Self {
        Self { variants }
    }

    pub fn at(&self, unix_seconds: i64) -> Site {
        let variant = self
            .variants
            .iter()
            .rev()
            .find(|v| v.from <= unix_seconds)
            .unwrap_or(&self.variants[0]);
        Site::new(variant.entries, variant.digest)
    }

    /// Scheduled moments still ahead of `unix_seconds`.
    pub fn upcoming(&self, unix_seconds: i64) -> usize {
        self.variants
            .iter()
            .filter(|v| v.from > unix_seconds)
            .count()
    }

    #[cfg(test)]
    pub fn all(&self) -> impl Iterator<Item = Site> + '_ {
        self.variants.iter().map(|v| Site::new(v.entries, v.digest))
    }
}

/// The wall clock as Unix seconds.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub trait SiteState {
    fn site(&self) -> Site;
}

impl SiteState for State {
    fn site(&self) -> Site {
        self.sites.at(now())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const FIXTURE: &[Entry] = &[
        Entry {
            path: "404.html",
            content_type: "text/html; charset=utf-8",
            hash: "404404",
            identity: b"<h1>not here</h1>",
            br: None,
            gzip: None,
        },
        Entry {
            path: "about.html",
            content_type: "text/html; charset=utf-8",
            hash: "aaaa",
            identity: b"about",
            br: None,
            gzip: None,
        },
        Entry {
            path: "assets/app-3f9a1c7e.css",
            content_type: "text/css; charset=utf-8",
            hash: "c55c55",
            identity: b"body{margin:0}",
            br: Some(b"BR"),
            gzip: Some(b"GZ"),
        },
        Entry {
            path: "docs/index.html",
            content_type: "text/html; charset=utf-8",
            hash: "d0c5",
            identity: b"docs",
            br: None,
            gzip: None,
        },
        Entry {
            path: "index.html",
            content_type: "text/html; charset=utf-8",
            hash: "1dex",
            identity: b"<h1>home</h1>",
            br: Some(b"BR-HOME"),
            gzip: Some(b"GZ-HOME"),
        },
    ];

    pub(crate) fn fixture() -> Site {
        Site::new(FIXTURE, "fixture-digest")
    }

    static FIXTURE_VARIANTS: &[Variant] = &[Variant {
        from: i64::MIN,
        entries: FIXTURE,
        digest: "fixture-digest",
    }];

    pub(crate) fn fixture_sites() -> Sites {
        Sites::new(FIXTURE_VARIANTS)
    }

    fn path_of(resolution: Resolution) -> Option<&'static str> {
        match resolution {
            Resolution::Found(entry) => Some(entry.path),
            _ => None,
        }
    }

    #[test]
    fn the_root_and_directories_serve_their_index_document() {
        let site = fixture();
        assert_eq!(path_of(site.resolve("/")), Some("index.html"));
        assert_eq!(path_of(site.resolve("/docs/")), Some("docs/index.html"));
    }

    #[test]
    fn a_page_is_found_with_or_without_its_html_extension() {
        let site = fixture();
        assert_eq!(path_of(site.resolve("/about")), Some("about.html"));
        assert_eq!(path_of(site.resolve("/about.html")), Some("about.html"));
    }

    #[test]
    fn a_directory_named_without_its_slash_redirects_to_the_slash_form() {
        assert_eq!(fixture().resolve("/docs"), Resolution::AddSlash);
    }

    #[test]
    fn dot_segments_never_escape_the_site_however_they_are_spelled() {
        let site = fixture();
        for path in [
            "/../Cargo.toml",
            "/assets/../index.html",
            "/%2e%2e/%2e%2e/etc/passwd",
            "/assets/%2E%2E%2Findex.html",
            "/assets/..%5cindex.html",
            "/./index.html",
            "//index.html",
            "/index.html%00",
            "/%ff",
            "relative",
        ] {
            assert_eq!(site.resolve(path), Resolution::NotFound, "{path}");
        }
    }

    #[test]
    fn a_percent_encoded_name_resolves_like_its_plain_spelling() {
        assert_eq!(path_of(fixture().resolve("/%61bout")), Some("about.html"));
    }

    #[test]
    fn brotli_wins_a_tie_with_gzip_and_a_higher_q_value_wins_outright() {
        let entry = fixture().get("index.html").unwrap();
        assert_eq!(
            entry.negotiate(Some("gzip, deflate, br")).0,
            Encoding::Brotli
        );
        assert_eq!(entry.negotiate(Some("br;q=0.5, gzip")).0, Encoding::Gzip);
    }

    #[test]
    fn a_coding_refused_with_q_zero_is_never_chosen() {
        let entry = fixture().get("index.html").unwrap();
        assert_eq!(
            entry.negotiate(Some("br;q=0, gzip;q=0")).0,
            Encoding::Identity
        );
        assert_eq!(entry.negotiate(Some("*;q=0")).0, Encoding::Identity);
    }

    #[test]
    fn a_wildcard_admits_the_compressed_variants() {
        let entry = fixture().get("index.html").unwrap();
        assert_eq!(entry.negotiate(Some("*")).0, Encoding::Brotli);
    }

    #[test]
    fn no_accept_encoding_or_no_variant_means_identity() {
        let site = fixture();
        assert_eq!(
            site.get("index.html").unwrap().negotiate(None).0,
            Encoding::Identity
        );
        assert_eq!(
            site.get("about.html").unwrap().negotiate(Some("br")).0,
            Encoding::Identity
        );
    }

    #[test]
    fn hashed_assets_are_immutable_and_everything_else_revalidates() {
        let site = fixture();
        assert!(
            site.get("assets/app-3f9a1c7e.css")
                .unwrap()
                .cache_control()
                .contains("immutable")
        );
        let html = site.get("index.html").unwrap().cache_control();
        assert!(html.contains("must-revalidate") && !html.contains("immutable"));
    }

    #[test]
    fn if_none_match_compares_weakly_and_honours_the_wildcard() {
        assert!(if_none_match_hits("\"abc\"", "\"abc\""));
        assert!(if_none_match_hits("W/\"abc\"", "\"abc\""));
        assert!(if_none_match_hits("\"x\", \"abc\"", "\"abc\""));
        assert!(if_none_match_hits("*", "\"abc\""));
        assert!(!if_none_match_hits("\"abc-br\"", "\"abc\""));
    }

    #[test]
    fn each_representation_has_its_own_validator() {
        let entry = fixture().get("index.html").unwrap();
        let tags = [Encoding::Brotli, Encoding::Gzip, Encoding::Identity].map(|e| entry.etag(e));
        assert_ne!(tags[0], tags[1]);
        assert_ne!(tags[1], tags[2]);
    }

    /// The site must work under the CSP in `api.rs`, which forbids inline
    /// script, inline style and event-handler attributes. Catch that here, when
    /// the designed site is dropped in, rather than in a browser console.
    #[test]
    fn the_embedded_html_needs_nothing_the_csp_forbids() {
        let all: Vec<Site> = [false, true]
            .into_iter()
            .flat_map(|drafts| {
                [false, true]
                    .into_iter()
                    .flat_map(move |nl| Sites::embedded(drafts, nl).all().collect::<Vec<_>>())
            })
            .collect();
        for entry in all.iter().flat_map(|site| site.entries()) {
            if !entry.content_type.starts_with("text/html") {
                continue;
            }
            let html = String::from_utf8_lossy(entry.identity).to_ascii_lowercase();
            let path = entry.path;
            assert!(
                !html.contains("<style"),
                "{path}: inline <style> is blocked by style-src 'self'"
            );
            assert!(
                !html.contains(" style="),
                "{path}: style= attributes are blocked by style-src 'self'"
            );
            for tag in html.split("<script").skip(1) {
                let open = tag.split('>').next().unwrap_or("");
                let data_only = open.contains("type=\"application/json\"")
                    || open.contains("type=\"application/ld+json\"");
                assert!(
                    open.contains("src=") || data_only,
                    "{path}: inline <script> is blocked by script-src 'self'"
                );
            }
            assert!(
                !has_event_handler(&html),
                "{path}: on*= event handler attributes are blocked by script-src 'self'"
            );
        }
    }

    /// True when `html` contains an attribute like ` onclick=`.
    fn has_event_handler(html: &str) -> bool {
        html.match_indices(|c: char| c.is_ascii_whitespace())
            .any(|(at, _)| {
                let rest = &html[at + 1..];
                let Some(name) = rest.strip_prefix("on") else {
                    return false;
                };
                let letters = name.bytes().take_while(u8::is_ascii_lowercase).count();
                letters > 0 && name[letters..].trim_start().starts_with('=')
            })
    }

    #[test]
    fn the_event_handler_check_finds_handlers_and_ignores_prose() {
        assert!(has_event_handler("<a onclick=\"x()\">"));
        assert!(has_event_handler("<body\nonload = 'x'>"));
        assert!(!has_event_handler("<p>built on grund</p>"));
    }

    /// Every same-origin link in the embedded HTML and CSS (`href="/…"`,
    /// `src="/…"`, `url("/…")`) names something the site serves. A renamed
    /// asset or a typo fails here, not as a broken page in production. A
    /// route the server answers itself (`api::SERVER_ROUTES`) counts too.
    #[test]
    fn every_local_link_in_the_embedded_site_resolves() {
        let mut broken = Vec::new();
        let all: Vec<Site> = [false, true]
            .into_iter()
            .flat_map(|drafts| {
                [false, true]
                    .into_iter()
                    .flat_map(move |nl| Sites::embedded(drafts, nl).all().collect::<Vec<_>>())
            })
            .collect();
        for site in all {
            for entry in site.entries() {
                let text_type = entry.content_type.starts_with("text/html")
                    || entry.content_type.starts_with("text/css");
                if !text_type {
                    continue;
                }
                let text = String::from_utf8_lossy(entry.identity);
                for opener in ["href=\"/", "src=\"/", "url(\"/"] {
                    for (at, _) in text.match_indices(opener) {
                        let rest = &text[at + opener.len() - 1..];
                        let target: String = rest.chars().take_while(|c| *c != '"').collect();
                        let path = target.split(['#', '?']).next().unwrap_or("");
                        if !crate::api::SERVER_ROUTES.contains(&path)
                            && !matches!(site.resolve(path), Resolution::Found(_))
                        {
                            broken.push(format!("{} -> {target}", entry.path));
                        }
                    }
                }
            }
        }
        assert!(
            broken.is_empty(),
            "links that resolve to nothing: {broken:?}"
        );
    }

    /// `curl -fsSL grund.sh/install | sh -s -- --domain …` must be
    /// harmless until grund is released: it says so, exits non-zero, and
    /// leaves the directory it runs in untouched. Runs the embedded bytes the
    /// way the pipe does, on stdin.
    #[test]
    fn the_short_install_path_serves_the_install_script() {
        let site = Site::embedded(false);
        assert_eq!(path_of(site.resolve("/install")), Some("install.sh"));
        assert_eq!(path_of(site.resolve("/install.sh")), Some("install.sh"));
    }

    #[test]
    fn the_install_script_changes_nothing_until_grund_is_released() {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let script = Site::embedded(false)
            .get("install.sh")
            .expect("install.sh is part of the site");
        assert!(script.content_type.starts_with("text/plain"));

        let dir = std::env::temp_dir().join(format!("grund-install-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new("sh")
            .args(["-s", "--", "--domain", "app.example.com"])
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("sh is available");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.identity)
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let left_behind = std::fs::read_dir(&dir).unwrap().count();
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(
            output.status.code(),
            Some(1),
            "must not report success: {stdout}"
        );
        assert!(stdout.contains("not available yet"), "{stdout}");
        assert!(stdout.contains("https://app.example.com"), "{stdout}");
        assert!(stdout.contains("Nothing was changed"), "{stdout}");
        assert_eq!(
            left_behind, 0,
            "the script wrote into its working directory"
        );
    }

    /// Drafts only ever add to the public site: every public file is served
    /// identically with drafts on, and nothing the public table lacks is a
    /// published post.
    #[test]
    fn the_drafts_site_is_the_public_site_plus_drafts() {
        let public = Site::embedded(false);
        let drafts = Site::embedded(true);
        for entry in public.entries() {
            let other = drafts.get(entry.path);
            let blog_listing = entry.path == "blog/index.html" || entry.path == "blog/feed.xml";
            assert!(other.is_some(), "{} is missing with drafts on", entry.path);
            if !blog_listing {
                assert_eq!(other.unwrap().hash, entry.hash, "{}", entry.path);
            }
        }
        for entry in drafts.entries() {
            if public.get(entry.path).is_none() && entry.path != "blog/feed.xml" {
                let html = String::from_utf8_lossy(entry.identity);
                assert!(
                    entry.path.starts_with("blog/") && html.contains("noindex"),
                    "{} is served only with drafts on, but is not a draft page",
                    entry.path
                );
            }
        }
    }

    /// Each scheduled moment only adds to the public site: every file of an
    /// earlier variant is still served, identically unless it is a blog
    /// listing, so a schedule can never take a page down.
    #[test]
    fn each_scheduled_variant_only_adds_to_the_one_before() {
        let variants: Vec<Site> = Sites::embedded(false, false).all().collect();
        for pair in variants.windows(2) {
            for entry in pair[0].entries() {
                let later = pair[1].get(entry.path);
                assert!(
                    later.is_some(),
                    "{} disappears at a scheduled moment",
                    entry.path
                );
                let listing = entry.path == "blog/index.html" || entry.path == "blog/feed.xml";
                if !listing {
                    assert_eq!(later.unwrap().hash, entry.hash, "{}", entry.path);
                }
            }
        }
    }

    static A: &[Entry] = &[];
    static B: &[Entry] = &[];
    static SCHEDULE: &[Variant] = &[
        Variant {
            from: i64::MIN,
            entries: A,
            digest: "before",
        },
        Variant {
            from: 1_000,
            entries: B,
            digest: "after",
        },
    ];

    #[test]
    fn the_variant_served_switches_exactly_at_its_moment() {
        let sites = Sites::new(SCHEDULE);
        assert_eq!(sites.at(999).digest(), "before");
        assert_eq!(sites.at(1_000).digest(), "after");
        assert_eq!(sites.at(i64::MAX).digest(), "after");
        assert_eq!(sites.upcoming(999), 1);
        assert_eq!(sites.upcoming(1_000), 0);
    }

    #[test]
    fn the_embedded_site_has_its_required_documents() {
        let site = Site::embedded(false);
        assert!(site.get("index.html").is_some());
        assert!(site.not_found_document().is_some());
        assert!(
            site.entries()
                .windows(2)
                .all(|pair| pair[0].path < pair[1].path),
            "table must be sorted"
        );
    }
}
