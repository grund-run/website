# Requirements record: grund.run server

Status: first designed version of the page in `site/`, hand-written, no build step.
This file records what the server guarantees, the contract the designed site
must meet, and what is deliberately not done. How to run it is in
[README.md](README.md).

## The site contract (for dropping in the designed site)

1. **Where the files go.** The finished site is a directory of static files at
   `site/`. It replaces the placeholder wholesale. A build may point elsewhere
   with `GRUND_WEBSITE_SITE_DIR=<dir> cargo build` (relative to the crate).
2. **How it gets there.** Today `site/` is committed as-is. When the design
   comes with a build tool, its sources go in `web/`, a CI step builds them
   into `site/` before `cargo build`, and `site/` becomes build output
   (gitignored). The Rust side does not change: it embeds whatever `site/`
   holds at compile time. *(The `web/` step is not built.)*
3. **Required files.** `index.html` and `404.html` at the root. The build
   fails without them.
4. **URLs map to files.**
   - `/` serves `index.html`.
   - `/dir/` serves `dir/index.html`, and `/dir` redirects 308 to `/dir/`.
   - `/page` serves `page` or `page.html`.
   - Anything else serves `404.html` with status 404.
5. **Hashed assets under `assets/`.** Every file under `assets/` is served
   `Cache-Control: public, max-age=31536000, immutable`, so its name must change
   when its content changes (`app-3f9a1c7e.css`). Vite does this by default;
   Astro needs `build.assets: "assets"`. Everything outside `assets/` is served
   `public, max-age=0, must-revalidate` with a strong ETag.
6. **File names.** ASCII letters, digits and `. _ - ~ @ +` only, so the URL and
   the file name are the same string. Dotfiles are ignored, except under
   `.well-known/`. No symlinks. The build fails naming the offending file.
7. **Content types.** Known extensions only (html, css, js, mjs, json, map,
   webmanifest, txt, xml, svg, png, jpg, jpeg, gif, webp, avif, ico, woff2,
   woff, wasm, pdf, mp4, webm). An unknown extension fails the build; add it to
   `content_type` in `build.rs` deliberately.
8. **Hand-written today.** Files that change (`styles.css`) live outside
   `assets/`, so editing them needs no renaming; they revalidate by ETag.
   Files that never change (the fonts) live in `assets/` under content-hashed
   names. Every same-origin `href`, `src` and CSS `url()` must resolve, which
   `cargo test` checks (`every_local_link_in_the_embedded_site_resolves`).
9. **It must run under the CSP.** Everything is loaded from this origin. That
   means no inline `<script>` (JSON data blocks excepted), no inline `<style>`
   or `style=` attributes, no `on*=` handlers, and no third-party fonts,
   scripts, analytics or embeds. `cargo test` scans every embedded HTML file for
   the first three
   (`the_embedded_html_needs_nothing_the_csp_forbids`). Loosening the CSP is a
   reviewed change to `CSP` in `src/api.rs`, with the reason in the commit.

## Guarantees

- **One origin.** Hosts in `GRUND_WEBSITE_REDIRECT_HOSTS` answer every request
  with 308 to the same path and query on `GRUND_WEBSITE_CANONICAL_ORIGIN`.
  Other hosts are served as-is, so probes that send a pod IP work.
- **No path escapes the site.** A request can only name an entry in a table
  compiled into the binary; there is no filesystem at runtime. `..`, `%2e%2e`,
  `\`, empty segments and NUL are 404.
- **Compression without request-time work.** Brotli (q11) and gzip variants
  are made at build time for text-like types of at least 256 bytes, and kept
  only when they save at least 10%. Negotiation honours q-values, including
  `q=0`. Each representation has its own ETag.
- **Conditional requests.** `If-None-Match` (weak comparison) answers 304.
- **HEAD** answers with the GET headers, including `Content-Length`, and no
  body. Methods other than GET and HEAD are 405 with `Allow: GET, HEAD`.
- **Security headers on every response**, including 404, 405, redirects,
  timeouts and panics: the CSP, `nosniff`, `Referrer-Policy:
  strict-origin-when-cross-origin`, `X-Frame-Options: DENY`, COOP and CORP
  `same-origin`, and a `Permissions-Policy` denying device APIs.
- **Deployment is provable.** `/health/ready` reports `revision` (the commit
  compiled in from `CI_COMMIT_SHA`) and `site_digest` (SHA-256 over every
  embedded path and hash).
- **Graceful shutdown.** SIGTERM drains in-flight requests for up to
  `GRUND_WEBSITE_SHUTDOWN_GRACE` (default 10 s, maximum 30 s, the kubelet's
  default grace period).

## Limits

| Limit | Default | Maximum | Env var | Reason |
|---|---|---|---|---|
| Request duration | 10 s | none | `GRUND_WEBSITE_REQUEST_TIMEOUT` | Responses are in-memory copies; anything longer is a stalled client |
| Shutdown grace | 10 s | 30 s | `GRUND_WEBSITE_SHUTDOWN_GRACE` | Must finish inside the kubelet's grace period |
| Site size | none enforced | the pod memory limit (64 MiB in `forest.cue`) | n/a | The site lives in the binary; identity, brotli and gzip copies all count |

## Deliberately not done

- **No server-side rendering yet.** The door is open: minijinja page routes go
  in `src/api.rs` before the static fallback, per the grund web-pages rules.
  Nothing needs it today.
- **No HSTS by default.** `GRUND_WEBSITE_HSTS_MAX_AGE` is 0. HSTS is a promise
  browsers keep for the whole max-age even if TLS breaks, so it is switched on
  only after HTTPS on every host of the origin is proven, starting small.
- **No `Last-Modified`.** Files have no meaningful modification time once
  embedded. The content-hash ETag is the validator.
- **No range requests.** Every file is small enough to send whole. Add them if
  the site ever ships video.
- **No runtime compression.** Everything compressible is precompressed. The
  server never compresses on the request path.
- **No 406.** A client refusing identity still gets identity, which RFC 9110
  permits.
- **No third-party fonts, scripts or analytics.** Inter and JetBrains Mono are
  self-hosted (SIL OFL 1.1, licenses served under `/licenses/`).
- **No CA bundle in the image.** The server makes no outbound connections.

## Verification

- `cargo test --locked`: 42 unit tests, and 16 accepttests
  (`tests/accepttest/`) against a spawned binary. The accepttests take
  `GRUND_WEBSITE_ACCEPT_URL` to run against the image or a live origin instead.
- The two records below were made with `ci/assert-http.sh` (62 checks), the
  shell contract that the accepttests replaced on the same day with the same
  coverage.
- Verified 2026-09-24 against the release binary (`cargo build --release`,
  glibc host) and, via `./check.sh`, against the static musl binary from
  `rust:1.98-alpine` in a read-only scratch container. 62/62 both times.
- Verified 2026-09-24 against dev on clank-dev (pod from image
  `git.kjuulh.io/grund/website:main-2c3562dadf25c7a4ca6bdbacb5b2c34d13f3e49b`,
  through `kubectl port-forward`): `/health/ready` reported `revision`
  `2c3562dadf25c7a4ca6bdbacb5b2c34d13f3e49b` and the same `site_digest` as the
  local build, and `EXPECT_NOINDEX=1 ci/assert-http.sh` passed 59/59.
- **Verified 2026-09-24 from the public internet against
  https://dev.grund.run**, revision `35654efbe5e97abe72f9ac04664746177f308de3`:
  - Let's Encrypt certificate for `CN=dev.grund.run`, valid to 2026-12-23;
  - `http://` answers 301 to `https://`;
  - the accepttests passed 16/16 with `GRUND_WEBSITE_ACCEPT_NOINDEX=true`.

  Through the real edge, Traefik refuses encoded `/` and `\` in a path with
  400 before the request reaches the server. The traversal test accepts that
  refusal as well as the server's own 404.
