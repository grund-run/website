# grund.run

The server behind [grund.run](https://grund.run). It serves a static site that
is built elsewhere and embedded into one static binary at compile time. The
binary runs on a `scratch` image with no filesystem, no database and no
outbound connections.

The site in `site/` today is a placeholder. The design is being done
separately. [REQUIREMENTS.md](REQUIREMENTS.md) states the contract the designed
site has to meet to drop in, and what the server guarantees.

## Run it

```bash
cargo run                                  # http://127.0.0.1:8080
cargo run -- --help                        # every knob, with its env var
```

Configuration is flags or environment variables. There are no config files.

| Env var | Default | What |
|---|---|---|
| `GRUND_WEBSITE_LISTEN` | `127.0.0.1:8080` | Bind address. The image sets `0.0.0.0:8080` |
| `GRUND_WEBSITE_CANONICAL_ORIGIN` | `https://grund.run` | The one origin; alias hosts redirect here |
| `GRUND_WEBSITE_REDIRECT_HOSTS` | empty | Comma-separated hosts that 308 to the canonical origin |
| `GRUND_WEBSITE_NOINDEX` | `false` | Send `X-Robots-Tag: noindex` (dev) |
| `GRUND_WEBSITE_HSTS_MAX_AGE` | `0` (off) | HSTS max-age in seconds |
| `GRUND_WEBSITE_REQUEST_TIMEOUT` | `10` | Seconds per request |
| `GRUND_WEBSITE_SHUTDOWN_GRACE` | `10` | Seconds to drain on SIGTERM, at most 30 |
| `GRUND_WEBSITE_LOG_FORMAT` | `compact` | `compact` or `json` (the image sets `json`) |
| `RUST_LOG` | `grund_website=info,notmad=info,info` | Log filter |

Endpoints:

- `GET /health/live`: `{"status":"ok"}`. Checks nothing.
- `GET /health/ready`: status, `revision` (the build commit), `site_digest`
  and the file count.
- Everything else: the site (see REQUIREMENTS.md for the URL mapping).

## Layout

```
build.rs              walks site/, hashes and precompresses every file, emits the table
site/                 the static site (placeholder today); the drop-in point
src/main.rs           config, tracing, notmad
src/config.rs         clap Config and its validation
src/site.rs           the embedded table: path resolution, encoding negotiation, cache policy
src/canonical.rs      alias host -> canonical origin redirects
src/api.rs            router, security headers, health, file responses
src/server.rs         the HTTP notmad component
ci/assert-http.sh     the HTTP contract, run by CI, check.sh and against live URLs
check.sh              builds the static binary in the CI image, packages it, asserts the container
Dockerfile.prebuilt   scratch image around the prebuilt binary
```

## Verify

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked                 # 41 tests
./check.sh                          # the above, plus the static binary in a read-only scratch container
```

Against any running instance:

```bash
ci/assert-http.sh http://127.0.0.1:8080 www.grund.run      # local: redirect host sent as Host
```
