# grund.sh

The server behind [grund.sh](https://grund.sh), grund's main site. It serves a static site that
is built elsewhere and embedded into one static binary at compile time. The
binary runs on a `scratch` image with no filesystem and no database. Its
one outbound connection is optional: page views to grund insights, only when
`GRUND_WEBSITE_INSIGHTS_URL` is set.

`site/` holds the designed page: a feature page, hand-written in HTML and CSS
in the palette of the grund dashboard design
(`design/reference/dashboard-overview.png`), with self-hosted Inter and
JetBrains Mono.

Any version of the site has to meet this contract; `build.rs` and
`cargo test` enforce it:

- `index.html` and `404.html` exist at the root.
- File names use only ASCII letters, digits and `. _ - ~ @ +`, and have a
  known extension (`content_type` in `build.rs`). Dotfiles are ignored,
  except under `.well-known/`.
- Everything under `assets/` is cached for a year, so its name must change
  when its content does. Everything else revalidates by ETag.
- It runs under the CSP in `src/api.rs`: everything from this origin, no
  inline `<script>`, `<style>`, `style=` or `on*=`. Every same-origin link
  resolves.

The requirements record, agent notes and design research live in
[grund/grund-docs](https://git.kjuulh.io/grund/grund-docs/src/branch/main/website)
under `website/` (private).

## The blog

Posts are markdown files in `blog/posts/<slug>.md`, served at
`/blog/<slug>`, with an index at `/blog/` and an Atom feed at
`/blog/feed.xml`. `build.rs` renders them at build time with the templates in
`blog/templates/` (the rules are in `src/blog.rs`), so the binary still serves
only an embedded table. Every post starts with front matter:

```markdown
---
title: What the post is called
date: 2026-09-25
summary: One sentence for the index, the feed and link previews.
draft: true
---

The post, in CommonMark with tables. `#` headings become `h2`: the title is the page's `h1`.
```

- **Drafts** (`draft: true`) are served only where
  `GRUND_WEBSITE_BLOG_DRAFTS` is on (dev), with a draft banner and
  `noindex`. In prod a draft is a 404, and with no published post there is
  no blog at all. To publish, set `draft: false` (or remove the line) and
  ship. The nav links the blog from its own pages; add a link from the other
  pages with the first published post.
- **Scheduling.** `publish_at: 2026-10-01T09:00:00Z` (UTC only) makes a
  post public from that moment. `build.rs` renders the public site once
  before the first scheduled moment and once from each, and the server serves
  the newest rendering whose moment has passed. The post, its index entry and
  its feed entry appear on the first request after the moment. Nothing of the
  post is in what is served before it, and no rebuild or redeploy happens at
  the moment. The image must be deployed before then, and a promotion to prod
  is still a person's step. Where drafts are on, a scheduled post shows at
  once, marked "Scheduled". `/health/ready`'s `site_digest` changes at the
  moment, which is how to prove from outside that it happened.
- **Raw HTML in a post is shown as text,** never passed through, so a post
  cannot add script or style the CSP forbids. Images go in
  `site/assets/` under a content-hashed name, like every other asset.
- A post with bad front matter, an unknown template placeholder or a path a
  file under `site/` already has fails the build, naming the file.
- Preview: `GRUND_WEBSITE_BLOG_DRAFTS=true cargo run`, then
  http://127.0.0.1:8080/blog/.

## The newsletter sign-up

A plain form in the home page's closing section, between `<!-- newsletter -->`
and `<!-- /newsletter -->`. With `GRUND_WEBSITE_NEWSLETTER` off (the default),
the build's other table has that block removed and the routes answer 404, so
a deployment without it shows no form. With it on:

- `POST /newsletter` relays the form to grund insights in-cluster
  (`/v1/newsletter/subscriptions`), with the page's path and utm tags from
  `Referer` and the client address from the configured header, and answers
  303 to `/newsletter/thanks`, or to `/newsletter/error` when insights refuses
  it, is down or is slow (3 s). Never 500.
- The mailed link, `GET /newsletter/confirm?token=`, shows a button, and only
  its POST confirms, because mail scanners open links. Then
  `/newsletter/confirmed?token=` shows the unsubscribe link.
- `GET`/`POST /newsletter/unsubscribe` is the same shape, and the POST also
  takes an RFC 8058 one-click request (`?token=`).
- A form posted with another site's Origin is refused (403). The honeypot
  field and the rate limit are judged by insights, and nothing logs a body or
  an address.

The consent checkbox's value, `newsletter-2026-10`, names the wording beside
it: change the words, change the version. The wording is a draft until it is
approved.

## Run it

```bash
cargo run                                  # http://127.0.0.1:8080
cargo run -- --help                        # every knob, with its env var
```

Configuration is flags or environment variables. There are no config files.

| Env var | Default | What |
|---|---|---|
| `GRUND_WEBSITE_LISTEN` | `127.0.0.1:8080` | Bind address. The image sets `0.0.0.0:8080` |
| `GRUND_WEBSITE_CANONICAL_ORIGIN` | `https://grund.sh` | The one origin; alias hosts redirect here |
| `GRUND_WEBSITE_REDIRECT_HOSTS` | empty | Comma-separated hosts that 308 to the canonical origin |
| `GRUND_WEBSITE_NOINDEX` | `false` | Send `X-Robots-Tag: noindex` (dev) |
| `GRUND_WEBSITE_BLOG_DRAFTS` | `false` | Serve blog drafts, marked and `noindex` (dev) |
| `GRUND_WEBSITE_NEWSLETTER` | `false` | Show the newsletter sign-up and serve its routes (needs `GRUND_WEBSITE_INSIGHTS_URL`) |
| `GRUND_WEBSITE_HSTS_MAX_AGE` | `0` (off) | HSTS max-age in seconds |
| `GRUND_WEBSITE_REQUEST_TIMEOUT` | `10` | Seconds per request |
| `GRUND_WEBSITE_SHUTDOWN_GRACE` | `10` | Seconds to drain on SIGTERM, at most 30 |
| `GRUND_WEBSITE_INSIGHTS_URL` | unset (off) | Base URL of grund insights' in-cluster ingest listener; page views are reported only when set |
| `GRUND_WEBSITE_INSIGHTS_CLIENT_IP_HEADER` | unset | Header carrying the client address as the edge saw it (`X-Real-Ip` behind Traefik) |
| `GRUND_WEBSITE_INSIGHTS_TOKEN` | unset | Bearer token, when insights requires one (a secret) |
| `GRUND_WEBSITE_INSIGHTS_SITE` | canonical host | Site name the views are reported under |
| `GRUND_WEBSITE_APP_URL` | unset | Origin of grund's dashboard; `/sign-in` sends visitors to its `/login` (302, `no-store`). Unset, `/sign-in` is a 404 |
| `GRUND_WEBSITE_LOG_FORMAT` | `compact` | `compact` or `json` (the image sets `json`) |
| `RUST_LOG` | `grund_website=info,notmad=info,info` | Log filter |

Endpoints:

- `GET /health/live`: `{"status":"ok"}`. Checks nothing.
- `GET /health/ready`: status, `revision` (the build commit), `site_digest`
  and the file count.
- `GET /sign-in`: a 302 to `$GRUND_WEBSITE_APP_URL/login`, never cached; a
  404 where no app URL is set. The nav's "Sign in" links here.
- Everything else: the site. `/` and `/dir/` serve `index.html` and
  `dir/index.html`, `/dir` 308s to `/dir/`, and `/page` serves `page`,
  `page.html` or `page.sh`. Anything else is `404.html` with status 404.

## Layout

```
build.rs              walks site/, hashes and precompresses every file, emits the table
blog/posts/           blog posts in markdown; blog/templates/ the post and index pages
site/                 the static site: index.html, pricing.html, licenses.html, 404.html, styles.css,
                      favicon.svg, favicon.ico, apple-touch-icon.png, assets/ (fonts, the mark,
                      the social card), licenses/ (full license texts)
design/reference/     the design the site follows
src/main.rs           config, tracing, notmad
src/config.rs         clap Config and its validation
src/site.rs           the embedded tables and which is served now: path resolution, encoding negotiation, cache policy
src/blog.rs           markdown posts to pages, index and feed (used by build.rs)
src/newsletter.rs     the newsletter form relay to grund insights; src/newsletter/*.html its token pages
src/canonical.rs      alias host -> canonical origin redirects
src/api.rs            router, security headers, health, file responses
src/server.rs         the HTTP notmad component
tests/accepttest/     the HTTP contract over the wire (given/when/then): spawned binary, image or live URL
check.sh              builds the static binary in the CI image, packages it, runs the accepttests on the container
Dockerfile.prebuilt   scratch image around the prebuilt binary
```

## Verify

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked                 # 71 unit tests + 29 accepttests against a spawned binary
./check.sh                          # the above, plus the accepttests against the static binary in a read-only scratch container
```

The accepttests (`tests/accepttest/`) spawn the binary by default. Point them at
any running instance instead:

```bash
# local: the redirect host is sent as the Host header
GRUND_WEBSITE_ACCEPT_URL=http://127.0.0.1:8080 GRUND_WEBSITE_ACCEPT_REDIRECT_HOST=www.grund.sh \
  cargo test --test tests
# live dev
GRUND_WEBSITE_ACCEPT_URL=https://dev.grund.sh GRUND_WEBSITE_ACCEPT_CANONICAL_ORIGIN=https://dev.grund.sh \
  GRUND_WEBSITE_ACCEPT_NOINDEX=true cargo test --test tests
# live prod: www is requested by name, so its DNS, route and certificate are proven too
GRUND_WEBSITE_ACCEPT_URL=https://grund.sh GRUND_WEBSITE_ACCEPT_REDIRECT_HOST=grund.run \
  cargo test --test tests
```

They speak raw HTTP/1.1 on purpose: traversal probes go out unnormalised, and
a HEAD body would be seen. Tests that need a specific server configuration
skip against an external target.

## Ship it

Woodpecker (`ci.git.kjuulh.io`, repo `grund/website`):

| Workflow | Runs on | Does |
|---|---|---|
| `.woodpecker/ci.yaml` | push to main, PR, manual | one workflow: fmt and clippy `-D warnings`, the unit tests and accepttests, and the static musl release build run side by side; the accepttest binary the tests built then runs against that release binary, and only then is that binary packaged with `Dockerfile.prebuilt`. PRs build the image without pushing; main pushes **one** tag, `git.kjuulh.io/grund/website:main-<sha>` |
| `.woodpecker/rollout.yaml` | after ci, main only | generated by `forest run install` (`kjuulh/woodpecker-forest`). Stages a forest release with that tag (`release prepare` and `annotate`) |

Releases go to the forest instance at **forest.kjuulh.io** (project
`kjuulh/grund-website`). The CLI and CI use its API endpoint,
`https://api.forest.kjuulh.io`. Where a staged release goes is decided by the
forest project's triggers, not by CI. Dev is automatic. **Production is promoted by a person**, after
checking the image by digest and running the accepttests against dev.

`forest.cue` deploys through the `kjuulh` organisation's Flux destinations on
the existing homelab clusters, via `kjuulh/kubernetes-app`:

| Env | Host | Replicas | Notes |
|---|---|---|---|
| dev | `dev.grund.sh` (+ `dev.grund.run`, redirected) | 1 | `noindex` |
| prod | `grund.sh` (+ `www.grund.sh`, `grund.run`, `www.grund.run`, redirected) | 2 | grund.run becomes the apps domain |

## Getting traffic here

`DNS -> public gateway (TLS passthrough) -> cluster ingress -> this pod`.
TLS terminates in the cluster, with a certificate per host, so the gateway
never sees plaintext. DNS and gateway configuration live outside this
repository.
