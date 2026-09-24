# CLAUDE.md: grund/website

Operational notes for agents. What the server promises is in REQUIREMENTS.md;
how to run it is in README.md.

## This repository is public

`grund/website` is public on git.kjuulh.io and push-mirrored hourly to
github.com/grund-run. Nothing sensitive goes in it: no secrets, tokens,
internal IPs, cluster names beyond what forest.cue needs, or gateway details.
Those live in the private `grund/terraform` repo, in forest config, or in
`kjuulh/clank-homelab`.

## Layout

- `site/` is the drop-in point for the designed site. `build.rs` embeds it.
  Change the site, rebuild. There is no runtime file access.
- `src/site.rs` holds all path, encoding and cache decisions, and is pure.
  `src/api.rs` only maps them to HTTP.
- `src/api.rs` holds `CSP`. Loosening it is a reviewed change, never config.

## Verify

```bash
cargo fmt --all --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked
./check.sh          # needs docker; builds in rust:1.98-alpine exactly as CI does
```

## Gotchas

- **`build.rs` panics on purpose** when `site/` lacks `index.html` or
  `404.html`, holds an unknown extension, or has a file name outside
  `[A-Za-z0-9._~@+-]`. The message names the file. Fix the site, not the check.
- **Anything under `site/assets/` is cached for a year.** Never put an unhashed
  file there.
- **`the_embedded_html_needs_nothing_the_csp_forbids` fails** when the site
  has inline script or style. Move it to a file under `assets/`.
- The revision in `/health/ready` comes from `CI_COMMIT_SHA` at build time.
  Local builds say `unknown`. Set `GRUND_WEBSITE_REVISION` to override.
- `check.sh` runs the in-container build as root and then chowns
  `target/musl` back to you. If you interrupt it, `target/musl` may be left
  owned by root.

## Open items

- The designed site (separate work). It drops into `site/` per REQUIREMENTS.md.
