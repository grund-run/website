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

- **grund.sh is the main site; grund.run is becoming the domain for customer
  apps** (Kasper, 2026-09-24; `design/app-domains.md`). Until the apps
  platform exists, grund.run, www.grund.run and www.grund.sh 308 to
  https://grund.sh, and dev.grund.run to https://dev.grund.sh. All the names
  of an environment share one Ingress and certificate (kubernetes-app 0.1.13
  `additional_hosts`).
- dev: https://dev.grund.sh, namespace `dev` on clank-dev
  (`~/.kube/clank-dev.yaml`). Public since 2026-09-24:
  - Cloudflare DNS from grund/terraform (both zones);
  - ratchet SNI routes from kjuulh/clank-homelab;
  - cert-manager DNS-01 through the grund solver (grund.sh, grund.run) in
    clank-homelab-flux.
- prod: https://grund.sh, namespace `prod` on clank-prod
  (`~/.kube/clank-prod.yaml`), 2 replicas. Live since 2026-09-24.
  - Promoted by hand, as Kasper cleared:
    `forest release release <slug> --environment prod`.
  - There is no prod trigger; each prod release is a deliberate promotion.
  - Forest's Flux webhook for clank-prod fails (non-fatal).
    `flux reconcile source git flux-system` makes it apply at once.
  - Reconcile only **after** `forest release show <slug>` reports
    `[prod] ... [SUCCEEDED]`. With `--no-wait`, a reconcile can run before
    forest has committed the release: Flux applies the previous revision,
    `rollout status` reports the old deployment as healthy, and prod stays
    on the old commit (happened 2026-09-24). Always confirm with the
    `revision` from https://grund.sh/health/ready.
  - www.grund.sh, grund.run and www.grund.run are served from the prod
    Ingress through kubernetes-app 0.1.13's `additional_hosts`, on the same
    certificate, and the server 308s them to https://grund.sh.
    forest-components' CI publishes
    each version to both forest servers since 2026-09-24 (it used to publish
    only to forest.i.kjuulh.io, which releases do not read).
  - Never `forest publish` from this machine: forest 0.3.13 sends it to
    https://api.forest.understory.sh whatever `--context` or
    `--forest-server` says (seen in a `-vv --dry-run`).
- Prove what is deployed: `curl -s https://dev.grund.sh/health/ready`. The
  `revision` must equal the commit. Then run the accepttests against it
  (README.md "Verify").

## Editing the site

- The palette and type come from `design/reference/dashboard-overview.png`,
  and the colour tokens are at the top of `site/styles.css`. Keep the page and
  the product looking like one thing. The dashboard is a colour reference
  only; it is not shown on the page.
- Icons are one inline SVG sprite at the top of `index.html`, used with
  `<use href="#id">`. The hero strata SVG is generated artwork, inlined in the
  page.
- Copy must not present planned guarantees as shipped. The evidence examples
  are labelled as examples.
- Look at it in a real browser with the real server, so the CSP applies:
  `cargo run`, then headless Chrome with `--screenshot` at 1440 px and 390 px
  wide. `google-chrome-stable` is on this machine.

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

- The page tells one story: bring a machine, deploy your app, it's live for
  your customers, for any app from a homelab to a SaaS. The tone is simple,
  boring and efficient (Kasper's direction, 2026-09-24).
- **No infrastructure lingo on the page.** No quorum, replicas, clusters,
  k3s, Flux, Postgres internals, PITR/RPO/RTO, digests, health gates,
  manifests, YAML, control plane or agents. Not even as negations ("no
  YAML"). The user deploys an application and never has to care what is
  underneath.
- **Releases and backups are features, not the product.** They are two tiles
  in the features grid ("Releases", "Data"), next to domains, logs, secrets
  and more machines. Do not bring back an evidence or drills section.
- Layout, in the boxes.dev mould:
  - hero over the drawn strata, then four facts;
  - three steps to live (machine, install, deploy) plus an "outgrow one
    machine?" note;
  - two examples (homelab, SaaS);
  - features, ownership, who it's for and the cost.
- Homelabs are in scope, which widens the audience beyond PLATFORM.md.
- The CLI commands are invented and captioned as illustrative.
- The hero shows `curl -fsSL grund.sh/install | sh -s -- --domain
  app.example.com`. `site/install.sh` is a real script, served as text/plain
  at `/install` and `/install.sh` (the resolver maps `/name` to `name.sh`).
  Until grund is released it only prints that grund is not available, exits
  1, and downloads, writes and changes nothing. Everything runs inside
  `main()`, so a truncated download runs nothing.
  `the_install_script_changes_nothing_until_grund_is_released` runs it the
  way the pipe does and enforces that. When grund ships, the real installer
  replaces it, and that test changes with it.
- grund is labelled in development in the first line, and examples are
  captioned as examples. The call to action is GitHub, because there is no
  signup backend.
- `/pricing` (`site/pricing.html`) shows **planned** pricing, labelled as
  such in its first line. The model is Kasper's (2026-09-24):
  - Self-hosted: open source, free, every part including the dashboard (grund
    is an open-source-first company; the site says so on the home page and on
    pricing, without naming a licence until one is chosen);
  - Homelab: first hosted machine free, then €3 per machine per month;
  - Pro: €10 per machine;
  - Business: €49 per month plus €10 per machine;
  - **grund machines** (rented through grund, priced by size: €29 / €45 /
    €79 / €109) carry no license fee.
  Running your own should stay cheap, like Tailscale. The machine prices are
  an Estimate at provider cost plus about €10; the cost basis and what is
  not established are in `design/pricing-research.md`. Price by the machine,
  never by traffic or app count.
- App addresses and custom domains are a planned feature. The design is
  `design/app-domains.md` (its own apps domain on the Public Suffix List,
  a relay for homelabs behind NAT, verified custom domains, grund-run
  zones, and the self-hosted equivalent with your own DNS provider). On the
  site it is "An address for every app", in plain words: no CNAME or zone
  talk.
- `/licenses` (`site/licenses.html`) holds the copyright notices and
  licenses for third-party material (the OFL fonts), with full texts under
  `site/licenses/`. Nowhere else on the site names them; the footer has one
  "Licenses" link. Add any new third-party asset there.

## Editing the site

- The palette and type come from `design/reference/dashboard-overview.png`,
  and the colour tokens are at the top of `site/styles.css`. Keep the page and
  the product looking like one thing. The dashboard is a colour reference
  only; it is not shown on the page.
- Icons are one inline SVG sprite at the top of `index.html`, used with
  `<use href="#id">`. The hero strata SVG is generated artwork, inlined in the
  page.
- Copy must not present planned guarantees as shipped. The evidence examples
  are labelled as examples.
- Look at it in a real browser with the real server, so the CSP applies:
  `cargo run`, then headless Chrome with `--screenshot` at 1440 px and 390 px
  wide. `google-chrome-stable` is on this machine.

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

- The page tells one story: zero to production on your own premises, for any
  app, homelab or SaaS (Kasper's direction, 2026-09-24). The layout is in the
  boxes.dev mould.
  - Hero over the drawn strata, then four facts.
  - The journey: levels 0 to 5, each with one small terminal or panel.
  - Two worked examples: a homelab on one mini PC, a SaaS on three servers.
  - Then evidence, ownership (including moving from the closet to Hetzner
    without a rewrite), who it is for, and the cost.
- This widens the audience beyond PLATFORM.md, which excluded hobbyists.
  Homelabs are now in scope, and the page says so.
- The CLI commands in the journey are invented and captioned as illustrative.
  The one real command is the install one-liner (see below).
- Every visual is captioned as an example; grund is labelled in development
  in the first line. Pricing is deliberately absent (still an estimate). The
  call to action is GitHub, because there is no signup backend.

## Editing the site

- The palette and type come from `design/reference/dashboard-overview.png`,
  and the colour tokens are at the top of `site/styles.css`. Keep the page and
  the product looking like one thing. The dashboard is a colour reference
  only; it is not shown on the page.
- Icons are one inline SVG sprite at the top of `index.html`, used with
  `<use href="#id">`. The hero strata SVG is generated artwork, inlined in the
  page.
- Copy must not present planned guarantees as shipped. The evidence examples
  are labelled as examples.
- Look at it in a real browser with the real server, so the CSP applies:
  `cargo run`, then headless Chrome with `--screenshot` at 1440 px and 390 px
  wide. `google-chrome-stable` is on this machine.

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

- The page is a feature page in the boxes.dev mould (Kasper's direction,
  2026-09-24):
  - The hero is a large headline over a drawn "ground strata" backdrop, not a
    product screenshot.
  - Then four short facts, and two-tone section headings (claim, then a
    grey qualifier).
  - Split sections pair a mono-labelled list with one focused visual: the
    evidence panel and the self-rolling-back release.
  - Then ownership cards with small visuals, and three steps.
  The copy is drafted from the platform plan and says plainly that grund is
  in development; every visual is captioned as an example. Pricing is
  deliberately absent (still an estimate). The call to action is GitHub,
  because there is no signup backend.
- HSTS is off (`GRUND_WEBSITE_HSTS_MAX_AGE=0`). Turn it on in prod, starting
  at 300, once https on grund.sh and every name that redirects to it is proven.
