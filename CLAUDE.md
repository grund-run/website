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

- Behaviour over the wire goes in `tests/accepttest/` as a given/when/then
  flow (the forest-server accepttest shape). Add a step to
  `fixtures/{given,when,then}.rs` when a flow needs one. Do not add shell
  assertion scripts.
- Accepttests must assert behaviour, not copy, so they survive the designed
  site dropping in (e.g. find the asset the home page links, never a
  hard-coded name).

## Deploy

- Push to main. CI publishes `git.kjuulh.io/grund/website:main-<sha>`, and
  `rollout.yaml` stages a forest release (project `kjuulh/grund-website`).
  Forest triggers roll it to dev. The project has one trigger, `main-to-dev`
  (branch `^main$`, environment `dev` only). There is no prod trigger.
- **Never promote to prod from here** (`forest release release/approve`
  against prod). That is Kasper's call.
- `rollout.yaml` is generated. After changing the `woodpecker-forest` block in
  `forest.cue`, run `forest run install` and commit what it writes. Do not
  edit the file by hand.
- **The forest instance is forest.kjuulh.io** (web UI at
  https://forest.kjuulh.io). The CLI and CI talk to its gRPC endpoint,
  `https://api.forest.kjuulh.io`. The web host itself returns an HTML 404 to
  gRPC. On this machine that instance is the `kjuulh-prod` context. Pass
  `--context kjuulh-prod` (or `FOREST_CONTEXT=kjuulh-prod`) on every command,
  because the default context points at a different forest instance.
- `forest validate` reports "Validated 0 component(s)", as it does for
  tiny-web. To check the config against `kubernetes-app`'s `#Spec`, run
  `cue vet` against the component's `forest.component.cue`.

## Live

- dev: https://dev.grund.run, namespace `dev` on clank-dev
  (`~/.kube/clank-dev.yaml`)
- prod: https://grund.run, namespace `prod` on clank-prod
  (`~/.kube/clank-prod.yaml`)
- Prove what is deployed: `curl -s https://dev.grund.run/health/ready`. The
  `revision` must equal the commit. Then run the accepttests against it
  (README.md "Verify").

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
- **www.grund.run has no Ingress or certificate yet.** `kubernetes-app`
  0.1.12 renders a single `host`. The server already redirects www
  (`GRUND_WEBSITE_REDIRECT_HOSTS`). What is missing is a component field for
  additional hosts, added to the Certificate's `dnsNames` and the Ingress
  rules and TLS hosts, then a bump here.
- HSTS is off (`GRUND_WEBSITE_HSTS_MAX_AGE=0`). Turn it on in prod, starting
  at 300, once https on grund.run and www.grund.run is proven.
