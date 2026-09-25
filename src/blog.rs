//! The blog: markdown posts under `blog/posts/`, rendered to HTML at build
//! time.
//!
//! `build.rs` includes this file with `#[path]`, reads every post, and embeds
//! what `render` returns beside the files under `site/`. The crate compiles it
//! only for its unit tests. Everything here is pure: strings in, strings out.
//!
//! A post is a file `blog/posts/<slug>.md` that starts with a front matter
//! block:
//!
//! ```text
//! ---
//! title: Our gateway broke every site for browsers for 8 hours
//! date: 2026-09-25
//! summary: One sentence for the index, the feed and link previews.
//! draft: true
//! ---
//! ```
//!
//! `draft` is optional and defaults to false. A draft is left out of the
//! public build: it is served only when GRUND_WEBSITE_BLOG_DRAFTS is on (dev),
//! with a draft banner and `noindex`. With no published post, the public
//! build has no blog at all.
//!
//! Markdown is CommonMark plus tables and strikethrough. Raw HTML in a post is
//! shown as text, never passed through: the site's CSP allows no inline
//! script or style, and a post must not be able to add either. Headings move
//! down one level, because the post title is the page's `h1`.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};

pub struct Post {
    pub slug: String,
    pub title: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    pub summary: String,
    pub draft: bool,
    pub body_html: String,
}

/// The two page templates, from `blog/templates/`. Placeholders are
/// `{{name}}`; `render` fills every one and refuses a template that leaves
/// one unfilled.
pub struct Templates<'a> {
    pub post: &'a str,
    pub index: &'a str,
}

/// One generated file: its URL path (as a site path, no leading slash) and
/// its bytes.
pub struct File {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// The canonical origin, for the feed and link previews, which need absolute
/// URLs. Dev serves the same bytes, so its feed points at production.
pub const ORIGIN: &str = "https://grund.sh";

/// Parses one post. `slug` is the file name without `.md`.
pub fn parse(slug: &str, source: &str) -> Result<Post, String> {
    if slug.is_empty()
        || !slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(format!(
            "{slug:?}: a post's file name must be lowercase letters, digits and -"
        ));
    }
    let rest = source
        .strip_prefix("---\n")
        .ok_or("the post must start with a --- front matter line")?;
    let (front, body) = rest
        .split_once("\n---\n")
        .ok_or("the front matter has no closing --- line")?;

    let (mut title, mut date, mut summary, mut draft) = (None, None, None, false);
    for line in front.lines().filter(|l| !l.trim().is_empty()) {
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| format!("front matter line {line:?} is not `key: value`"))?;
        let value = value.trim().to_string();
        match key.trim() {
            "title" => title = Some(value),
            "date" => date = Some(value),
            "summary" => summary = Some(value),
            "draft" => {
                draft = match value.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(format!("draft must be true or false, not {value:?}")),
                }
            }
            other => return Err(format!("unknown front matter key {other:?}")),
        }
    }
    let title = title
        .filter(|t| !t.is_empty())
        .ok_or("front matter needs a title")?;
    let summary = summary
        .filter(|s| !s.is_empty())
        .ok_or("front matter needs a summary")?;
    let date = date.ok_or("front matter needs a date")?;
    if !valid_date(&date) {
        return Err(format!("date must be YYYY-MM-DD, not {date:?}"));
    }
    Ok(Post {
        slug: slug.to_string(),
        title,
        date,
        summary,
        draft,
        body_html: markdown(body),
    })
}

/// Everything the blog adds to the site, for one build: the posts, the index
/// at `blog/index.html` and the Atom feed at `blog/feed.xml`. With
/// `include_drafts` false, drafts are left out, and with no post left there
/// is no blog at all. Posts are listed newest first.
pub fn render(
    posts: &[Post],
    templates: &Templates,
    include_drafts: bool,
) -> Result<Vec<File>, String> {
    let mut shown: Vec<&Post> = posts
        .iter()
        .filter(|p| include_drafts || !p.draft)
        .collect();
    if shown.is_empty() {
        return Ok(Vec::new());
    }
    shown.sort_by(|a, b| b.date.cmp(&a.date).then(a.slug.cmp(&b.slug)));

    let mut files = Vec::new();
    for post in &shown {
        let page = fill(
            templates.post,
            &[
                ("title", &escape(&post.title)),
                ("summary", &escape(&post.summary)),
                ("slug", &post.slug),
                ("date_iso", &post.date),
                ("date", &human_date(&post.date)),
                ("robots", robots(post.draft)),
                ("draft_banner", draft_banner(post.draft)),
                ("content", &post.body_html),
            ],
        )
        .map_err(|e| format!("blog/templates/post.html: {e}"))?;
        files.push(File {
            path: format!("blog/{}.html", post.slug),
            bytes: page.into_bytes(),
        });
    }

    let items: String = shown.iter().map(|p| index_item(p)).collect();
    let any_draft = shown.iter().any(|p| p.draft);
    let index = fill(
        templates.index,
        &[("robots", robots(any_draft)), ("posts", &items)],
    )
    .map_err(|e| format!("blog/templates/index.html: {e}"))?;
    files.push(File {
        path: "blog/index.html".into(),
        bytes: index.into_bytes(),
    });
    files.push(File {
        path: "blog/feed.xml".into(),
        bytes: feed(&shown).into_bytes(),
    });
    Ok(files)
}

fn markdown(body: &str) -> String {
    let parser = Parser::new_ext(body, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH).map(
        |event| match event {
            Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
            Event::Start(Tag::Heading {
                level,
                id,
                classes,
                attrs,
            }) => Event::Start(Tag::Heading {
                level: demote(level),
                id,
                classes,
                attrs,
            }),
            Event::End(TagEnd::Heading(level)) => Event::End(TagEnd::Heading(demote(level))),
            other => other,
        },
    );
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

fn demote(level: HeadingLevel) -> HeadingLevel {
    match level {
        HeadingLevel::H1 => HeadingLevel::H2,
        HeadingLevel::H2 => HeadingLevel::H3,
        HeadingLevel::H3 => HeadingLevel::H4,
        HeadingLevel::H4 => HeadingLevel::H5,
        _ => HeadingLevel::H6,
    }
}

fn index_item(post: &Post) -> String {
    let tag = if post.draft {
        r#" <span class="draft-tag">Draft</span>"#
    } else {
        ""
    };
    format!(
        "<li class=\"post-item\"><a href=\"/blog/{slug}\"><time datetime=\"{iso}\">{date}</time>\
         <span class=\"post-title\">{title}{tag}</span></a><p>{summary}</p></li>\n",
        slug = post.slug,
        iso = post.date,
        date = human_date(&post.date),
        title = escape(&post.title),
        summary = escape(&post.summary),
    )
}

fn feed(posts: &[&Post]) -> String {
    let updated = posts.first().map_or("", |p| p.date.as_str());
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <feed xmlns=\"http://www.w3.org/2005/Atom\">\n\
         <title>grund blog</title>\n\
         <id>{ORIGIN}/blog/</id>\n\
         <link rel=\"alternate\" href=\"{ORIGIN}/blog/\"/>\n\
         <link rel=\"self\" href=\"{ORIGIN}/blog/feed.xml\"/>\n\
         <updated>{updated}T00:00:00Z</updated>\n\
         <author><name>grund</name></author>\n"
    );
    for post in posts {
        out.push_str(&format!(
            "<entry>\n<title>{title}</title>\n<id>{ORIGIN}/blog/{slug}</id>\n\
             <link rel=\"alternate\" href=\"{ORIGIN}/blog/{slug}\"/>\n\
             <published>{date}T00:00:00Z</published>\n<updated>{date}T00:00:00Z</updated>\n\
             <summary>{summary}</summary>\n<content type=\"html\">{content}</content>\n</entry>\n",
            title = escape(&post.title),
            slug = post.slug,
            date = post.date,
            summary = escape(&post.summary),
            content = escape(&post.body_html),
        ));
    }
    out.push_str("</feed>\n");
    out
}

fn robots(draft: bool) -> &'static str {
    if draft {
        r#"<meta name="robots" content="noindex">"#
    } else {
        ""
    }
}

fn draft_banner(draft: bool) -> &'static str {
    if draft {
        r#"<p class="pill draft-banner"><span class="dot"></span> Draft. Not published: visible only where drafts are turned on.</p>"#
    } else {
        ""
    }
}

/// Replaces every `{{name}}` in `template`. An unknown or unfilled
/// placeholder is an error, so a typo in a template fails the build.
fn fill(template: &str, values: &[(&str, &str)]) -> Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").ok_or("an opening {{ has no closing }}")?;
        let name = after[..end].trim();
        let value = values
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
            .ok_or_else(|| format!("unknown placeholder {{{{{name}}}}}"))?;
        out.push_str(value);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn valid_date(date: &str) -> bool {
    let parts: Vec<&str> = date.split('-').collect();
    let [year, month, day] = parts.as_slice() else {
        return false;
    };
    let number = |s: &str, len: usize| {
        (s.len() == len && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse::<u32>().ok())
    };
    matches!(number(year, 4), Some(Some(_)))
        && matches!(number(month, 2), Some(Some(1..=12)))
        && matches!(number(day, 2), Some(Some(1..=31)))
}

/// `2026-09-25` as `25 September 2026`.
fn human_date(date: &str) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let mut parts = date.split('-');
    let (Some(year), Some(month), Some(day)) = (parts.next(), parts.next(), parts.next()) else {
        return date.to_string();
    };
    let month = month
        .parse::<usize>()
        .ok()
        .and_then(|m| MONTHS.get(m.wrapping_sub(1)))
        .copied()
        .unwrap_or(month);
    format!("{} {month} {year}", day.trim_start_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATES: Templates = Templates {
        post: "{{robots}}<h1>{{title}}</h1>{{draft_banner}}<time datetime=\"{{date_iso}}\">{{date}}</time><p>{{summary}}</p><a href=\"/blog/{{slug}}\"></a>{{content}}",
        index: "{{robots}}<ul>{{posts}}</ul>",
    };

    fn post(front: &str, body: &str) -> Result<Post, String> {
        parse("a-post", &format!("---\n{front}\n---\n{body}"))
    }

    fn published(slug: &str, date: &str) -> Post {
        let mut p = post(&format!("title: {slug}\ndate: {date}\nsummary: s"), "Body").unwrap();
        p.slug = slug.into();
        p
    }

    fn page<'a>(files: &'a [File], path: &str) -> &'a str {
        let file = files.iter().find(|f| f.path == path).expect(path);
        std::str::from_utf8(&file.bytes).unwrap()
    }

    #[test]
    fn a_post_renders_its_markdown_under_its_title() {
        let p = post(
            "title: Hello & welcome\ndate: 2026-09-25\nsummary: One line.",
            "# Why\n\nSome *words* and a [link](/pricing).\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
        )
        .unwrap();
        assert!(!p.draft);
        assert!(p.body_html.contains("<h2>Why</h2>"), "{}", p.body_html);
        assert!(p.body_html.contains("<em>words</em>"));
        assert!(p.body_html.contains("<table>"));

        let files = render(&[p], &TEMPLATES, false).unwrap();
        let html = page(&files, "blog/a-post.html");
        assert!(html.contains("<h1>Hello &amp; welcome</h1>"), "{html}");
        assert!(html.contains("25 September 2026"));
        assert!(!html.contains("noindex"));
    }

    #[test]
    fn raw_html_in_a_post_is_shown_as_text_not_run() {
        let p = post(
            "title: t\ndate: 2026-09-25\nsummary: s",
            "<script>alert(1)</script>\n\nInline <b onclick=\"x()\">bold</b>.\n",
        )
        .unwrap();
        assert!(!p.body_html.contains("<script"), "{}", p.body_html);
        assert!(!p.body_html.contains("<b "), "{}", p.body_html);
        assert!(p.body_html.contains("&lt;script&gt;"));
    }

    #[test]
    fn drafts_are_left_out_of_the_public_build_entirely() {
        let mut draft = published("draft-one", "2026-09-26");
        draft.draft = true;
        let posts = [published("old", "2026-09-01"), draft];

        let public = render(&posts, &TEMPLATES, false).unwrap();
        assert!(public.iter().all(|f| f.path != "blog/draft-one.html"));
        assert!(!page(&public, "blog/index.html").contains("draft-one"));
        assert!(!page(&public, "blog/feed.xml").contains("draft-one"));

        let with_drafts = render(&posts, &TEMPLATES, true).unwrap();
        let draft_page = page(&with_drafts, "blog/draft-one.html");
        assert!(draft_page.contains("noindex") && draft_page.contains("Draft."));
        let index = page(&with_drafts, "blog/index.html");
        assert!(index.contains("draft-tag") && index.contains("noindex"));
    }

    #[test]
    fn with_no_published_post_there_is_no_blog() {
        let mut draft = published("only", "2026-09-26");
        draft.draft = true;
        assert!(render(&[draft], &TEMPLATES, false).unwrap().is_empty());
    }

    #[test]
    fn the_index_and_feed_list_the_newest_post_first() {
        let posts = [
            published("older", "2026-01-02"),
            published("newer", "2026-03-04"),
        ];
        let files = render(&posts, &TEMPLATES, false).unwrap();
        let index = page(&files, "blog/index.html");
        assert!(index.find("newer").unwrap() < index.find("older").unwrap());
        let feed = page(&files, "blog/feed.xml");
        assert!(feed.contains("<updated>2026-03-04T00:00:00Z</updated>"));
        assert!(feed.contains("https://grund.sh/blog/newer"));
    }

    #[test]
    fn bad_front_matter_is_refused_with_the_reason() {
        let cases = [
            ("title: t\ndate: 2026-9-5\nsummary: s", "date"),
            ("date: 2026-09-05\nsummary: s", "title"),
            ("title: t\ndate: 2026-09-05", "summary"),
            (
                "title: t\ndate: 2026-09-05\nsummary: s\ndraft: yes",
                "draft",
            ),
            (
                "title: t\ndate: 2026-09-05\nsummary: s\nauthor: x",
                "unknown",
            ),
        ];
        for (front, needle) in cases {
            let error = post(front, "x").err().unwrap_or_default();
            assert!(error.contains(needle), "{front:?}: {error}");
        }
        assert!(parse("Bad_Name", "---\ntitle: t\n---\n").is_err());
        assert!(parse("a", "no front matter").is_err());
    }

    #[test]
    fn a_template_with_an_unknown_placeholder_fails_the_build() {
        let broken = Templates {
            post: "{{titel}}",
            index: TEMPLATES.index,
        };
        let error = render(&[published("a", "2026-01-01")], &broken, false)
            .err()
            .unwrap();
        assert!(error.contains("titel"), "{error}");
    }
}
