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
//! `publish_at: 2026-10-01T09:00:00Z` (UTC, optional) schedules a post. The
//! build renders the public blog once per scheduled time (`schedule`), and the
//! server serves the newest rendering whose time has passed, so the post, its
//! index entry and its feed entry all appear at that moment and not before.
//! Nothing of it is in what is served earlier. Where drafts are on, it shows
//! at once, marked as scheduled.
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
    /// Unix seconds (UTC) from which the post is public, when scheduled.
    pub publish_at: Option<i64>,
    pub body_html: String,
}

/// Which blog to render: the public one as it stands at a moment (Unix
/// seconds), or the one dev shows, with every draft and scheduled post.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Public { at: i64 },
    Drafts,
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
    let mut publish_at = None;
    for line in front.lines().filter(|l| !l.trim().is_empty()) {
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| format!("front matter line {line:?} is not `key: value`"))?;
        let value = value.trim().to_string();
        match key.trim() {
            "title" => title = Some(value),
            "date" => date = Some(value),
            "summary" => summary = Some(value),
            "publish_at" => {
                publish_at = Some(parse_utc(&value).ok_or_else(|| {
                    format!("publish_at must be UTC like 2026-10-01T09:00:00Z, not {value:?}")
                })?)
            }
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
        publish_at,
        body_html: markdown(body),
    })
}

/// The moments the public blog changes: every distinct `publish_at` of a
/// post that is not a draft, earliest first. The build renders the public
/// blog once before the first and once at each.
pub fn schedule(posts: &[Post]) -> Vec<i64> {
    let mut times: Vec<i64> = posts
        .iter()
        .filter(|p| !p.draft)
        .filter_map(|p| p.publish_at)
        .collect();
    times.sort_unstable();
    times.dedup();
    times
}

/// Everything the blog adds to the site, for one view: the posts, the index
/// at `blog/index.html` and the Atom feed at `blog/feed.xml`. The public view
/// leaves out drafts and posts scheduled after `at`, and with no post left
/// there is no blog at all. Posts are listed newest first.
pub fn render(posts: &[Post], templates: &Templates, view: View) -> Result<Vec<File>, String> {
    let mut shown: Vec<&Post> = posts
        .iter()
        .filter(|p| match view {
            View::Drafts => true,
            View::Public { at } => !p.draft && p.publish_at.is_none_or(|t| t <= at),
        })
        .collect();
    let marked = view == View::Drafts;
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
                ("draft_banner", &banner(post, marked)),
                ("content", &post.body_html),
            ],
        )
        .map_err(|e| format!("blog/templates/post.html: {e}"))?;
        files.push(File {
            path: format!("blog/{}.html", post.slug),
            bytes: page.into_bytes(),
        });
    }

    let items: String = shown.iter().map(|p| index_item(p, marked)).collect();
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

fn index_item(post: &Post, marked: bool) -> String {
    let tag = match (marked, post.draft, post.publish_at) {
        (_, true, _) => r#" <span class="draft-tag">Draft</span>"#,
        (true, false, Some(_)) => r#" <span class="draft-tag">Scheduled</span>"#,
        _ => "",
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

fn banner(post: &Post, marked: bool) -> String {
    match (post.draft, marked.then_some(post.publish_at).flatten()) {
        (true, _) => r#"<p class="pill draft-banner"><span class="dot"></span> Draft. Not published: visible only where drafts are turned on.</p>"#.into(),
        (false, Some(at)) => format!(
            r#"<p class="pill draft-banner"><span class="dot"></span> Scheduled. Public from {} UTC.</p>"#,
            human_time(at)
        ),
        _ => String::new(),
    }
}

/// `2026-10-01T09:00:00Z` or `2026-10-01T09:00Z` as Unix seconds. UTC only:
/// a schedule in someone's local time would move with the server's zone.
fn parse_utc(text: &str) -> Option<i64> {
    let (date, time) = text.strip_suffix('Z')?.split_once('T')?;
    if !valid_date(date) {
        return None;
    }
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (year, month, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let parts: Vec<&str> = time.split(':').collect();
    if !(parts.len() == 2 || parts.len() == 3)
        || parts
            .iter()
            .any(|p| p.len() != 2 || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let field = |i: usize| parts.get(i).map_or(Some(0), |p| p.parse::<i64>().ok());
    let (hour, minute, second) = (field(0)?, field(1)?, field(2)?);
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds as `1 October 2026, 09:00`.
fn human_time(at: i64) -> String {
    let days = at.div_euclid(86_400);
    let secs = at.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{}, {:02}:{:02}",
        human_date(&format!("{year:04}-{month:02}-{day:02}")),
        secs / 3_600,
        secs % 3_600 / 60
    )
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

    const PUBLIC: View = View::Public { at: i64::MAX };

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

        let files = render(&[p], &TEMPLATES, PUBLIC).unwrap();
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

        let public = render(&posts, &TEMPLATES, PUBLIC).unwrap();
        assert!(public.iter().all(|f| f.path != "blog/draft-one.html"));
        assert!(!page(&public, "blog/index.html").contains("draft-one"));
        assert!(!page(&public, "blog/feed.xml").contains("draft-one"));

        let with_drafts = render(&posts, &TEMPLATES, View::Drafts).unwrap();
        let draft_page = page(&with_drafts, "blog/draft-one.html");
        assert!(draft_page.contains("noindex") && draft_page.contains("Draft."));
        let index = page(&with_drafts, "blog/index.html");
        assert!(index.contains("draft-tag") && index.contains("noindex"));
    }

    #[test]
    fn with_no_published_post_there_is_no_blog() {
        let mut draft = published("only", "2026-09-26");
        draft.draft = true;
        assert!(render(&[draft], &TEMPLATES, PUBLIC).unwrap().is_empty());
    }

    #[test]
    fn the_index_and_feed_list_the_newest_post_first() {
        let posts = [
            published("older", "2026-01-02"),
            published("newer", "2026-03-04"),
        ];
        let files = render(&posts, &TEMPLATES, PUBLIC).unwrap();
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
    fn a_scheduled_post_is_public_from_its_moment_and_not_a_second_before() {
        let mut scheduled = published("soon", "2026-10-01");
        scheduled.publish_at = parse_utc("2026-10-01T09:00:00Z");
        let at = scheduled.publish_at.unwrap();
        let posts = [published("now", "2026-09-01"), scheduled];

        assert_eq!(schedule(&posts), vec![at]);
        let before = render(&posts, &TEMPLATES, View::Public { at: at - 1 }).unwrap();
        assert!(before.iter().all(|f| f.path != "blog/soon.html"));
        assert!(!page(&before, "blog/index.html").contains("soon"));
        assert!(!page(&before, "blog/feed.xml").contains("soon"));

        let after = render(&posts, &TEMPLATES, View::Public { at }).unwrap();
        let html = page(&after, "blog/soon.html");
        assert!(!html.contains("Scheduled") && !html.contains("noindex"));
        assert!(page(&after, "blog/index.html").contains("soon"));

        let dev = render(&posts, &TEMPLATES, View::Drafts).unwrap();
        assert!(
            page(&dev, "blog/soon.html")
                .contains("Scheduled. Public from 1 October 2026, 09:00 UTC.")
        );
        assert!(page(&dev, "blog/index.html").contains(">Scheduled<"));
    }

    #[test]
    fn publish_at_must_be_utc_and_real() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2026-10-01T09:00Z"), Some(1_790_845_200));
        assert_eq!(human_time(1_790_845_200), "1 October 2026, 09:00");
        for bad in [
            "2026-10-01T09:00:00",
            "2026-10-01 09:00Z",
            "2026-10-01T25:00Z",
            "2026-10-01T09:00+02:00",
        ] {
            assert_eq!(parse_utc(bad), None, "{bad}");
        }
        let error = post(
            "title: t\ndate: 2026-09-05\nsummary: s\npublish_at: tomorrow",
            "x",
        )
        .err()
        .unwrap_or_default();
        assert!(error.contains("publish_at"), "{error}");
    }

    #[test]
    fn a_draft_is_never_scheduled_into_the_public_blog() {
        let mut draft = published("wip", "2026-10-01");
        draft.draft = true;
        draft.publish_at = Some(0);
        assert!(schedule(std::slice::from_ref(&draft)).is_empty());
        assert!(render(&[draft], &TEMPLATES, PUBLIC).unwrap().is_empty());
    }

    #[test]
    fn a_template_with_an_unknown_placeholder_fails_the_build() {
        let broken = Templates {
            post: "{{titel}}",
            index: TEMPLATES.index,
        };
        let error = render(&[published("a", "2026-01-01")], &broken, PUBLIC)
            .err()
            .unwrap();
        assert!(error.contains("titel"), "{error}");
    }
}
