# Design: an address for every app

Status: **proposed design, not built.** Kasper's idea (2026-09-24), agreed
in principle. The decisions marked *Decision for Kasper* are open.

## What exists today

Nothing in the product: grund is in development. The only DNS grund runs is
its own zone, grund.run, in `grund/terraform` (`stacks/cloudflare`). There,
cert-manager issues per-host certificates by DNS-01 through a
Cloudflare solver scoped to grund.run (clank-homelab-flux `6ce4241`).

## The idea in one line

**Deploying an app gives it a working HTTPS address without the user
touching DNS. When they want their own domain, grund can verify it, or run
the whole zone for them.**

## 1. App addresses on a grund apps domain

`<app>-<account>.<apps-domain>`, for example
`photos-kasper.grundapps.net`. It is free on every hosted plan, including
Homelab's free machine.

### Decision: a separate domain, not `*.apps.grund.run`

Customer content under the product domain puts grund.run at risk:

- **Reputation.** One phishing app can get the registrable domain flagged by
  Google Safe Browsing or mail and DNS blocklists. That would take grund.run,
  the dashboard and `install.sh` down with it.
- **Cookies.** Without a Public Suffix List entry, any `*.apps.grund.run`
  page can set cookies for `grund.run`, where the dashboard session lives.
- **Certificate rate limits.** Let's Encrypt limits certificates per
  registered domain; app hostnames would compete with grund.run's own.

Established practice separates the two: `github.io` / `github.com`,
`vercel.app`, `fly.dev`, `netlify.app`. NAMING.md already recommends a
separate customer-app domain on the PSL.

### Decision: a wildcard certificate, flat names

- One `*.<apps-domain>` certificate by DNS-01, since grund controls that
  zone, instead of a certificate per app. Issuance is then independent of
  the number of apps.
- Names are flat (`<app>-<account>`), not nested
  (`<app>.<account>.…`). A wildcard covers exactly one label, and a nested
  scheme would need a certificate per account.
- The apps zone's CAA allows `issuewild` for the CA. grund.run keeps
  forbidding wildcards.

### Decision: register the domain on the Public Suffix List

Each app becomes its own registrable domain, so cookies and most reputation
decisions stay per app. PSL inclusion is a pull request with verification
and takes weeks. Start it as soon as the domain is registered.

### Candidate domains (RDAP, 2026-09-24; nothing registered)

| Domain | Status | Notes |
|---|---|---|
| grundapps.net | unregistered | Descriptive and conventional. Our recommendation |
| grundapps.com | unregistered | Register alongside, to stop squatting |
| grund.page | unregistered | Short; `.page` is HSTS-preloaded (always HTTPS) and run by Google Registry |
| grundusercontent.com | unregistered | Clear but long; suits raw content better than apps |
| grund.site, grund.host, grund.pub, grund.zone, grund.onl | unregistered | Cheap TLDs with weaker reputation and more corporate blocklisting. Avoid |
| grund.network | unregistered | Possible home for nameserver hostnames |
| grund.app, grund.live, grunddns.net | registered by others | — |

Prices were not checked; confirm at checkout, including premium pricing.
*Decision for Kasper:* which domain.

## 2. Reaching the app

DNS alone reaches only machines with a public address. Many homelabs sit
behind NAT or carrier-grade NAT with no inbound ports. So there are two
levels:

| Level | How | Cost to grund | Plans |
|---|---|---|---|
| Direct | The record points at the machine's public IP; the user forwards ports | ~nothing (DNS) | all hosted plans |
| Relay | The machine dials out to a grund edge and traffic returns through it, as with Cloudflare Tunnel or Tailscale Funnel | bandwidth | small free allowance on Homelab; included on Pro and Business within fair use |

The relay is the only part of this whose cost grows with traffic rather
than with machines. It needs a stated allowance before launch.
*Decision for Kasper:* the allowance.

## 3. Custom domains

### Layer 1: a specific domain (Pro and above)

1. The customer adds `CNAME app.customer.com → <app>-<account>.<apps-domain>`.
2. They add `TXT _grund.app.customer.com → <verification token>`. This
   proves the domain is theirs, not just pointed at us.
3. grund issues the certificate at its edge. Behind a proxy or relay, the
   customer delegates the challenge once with `CNAME
   _acme-challenge.app.customer.com → <id>.acme.<apps-domain>`, and grund
   answers DNS-01 in its own zone. Renewals then need nothing further.

**Takeover protection.** A hostname binds to the account that verified it.
If the customer deletes the app but leaves the CNAME, nobody else on grund
can attach `app.customer.com` without passing the TXT check themselves.
Released hostnames are held for a cool-down period before they can be
verified by anyone.

### Layer 2: grund runs the zone (Pro and above; DNSSEC and audit log on Business)

The customer delegates a zone, or a subdomain such as `apps.customer.com`,
to grund's nameservers. Deploying an app then creates its records, and
removing it removes them. The customer can still add their own records
(mail, verification TXTs).

- The nameservers need hostnames on a domain grund controls, for example
  `ns1.grund.run` or `ns1.grund.network`.
- Build or buy: run authoritative DNS ourselves (PowerDNS or Knot on a few
  machines, with anycast later), or start on a provider API (Hetzner DNS,
  Cloudflare for SaaS) behind one interface and move later.
  *Decision for Kasper;* we lean towards starting on a provider.
- DNSSEC signing and the DS handover belong to this layer.

## 4. Self-hosted

Open source first: the self-hosted edition does all of this **with the
user's own DNS provider**, through its API (Cloudflare, Hetzner DNS, Route 53
and others): app records, certificates, custom domains and zones. What only
the hosted plans add is grund's apps domain, the relay, and grund-run
nameservers. "You can always run all of it yourself" stays true.

## 5. Abuse

Free public URLs attract phishing and malware from the first week.

- An abuse contact (`abuse@` on a domain with mail, and a form) and a
  documented takedown path, with evidence retained.
- Rate limits on new apps and new public hostnames per account, stricter
  on free accounts.
- Monitoring of the apps domain against Google Safe Browsing and major
  blocklists, alerting on any flag.
- Suspension that stops serving a hostname without deleting the user's
  data.

Deliberately not planned: content scanning of apps.

## 6. What this design does not claim

- **No uptime promise for the relay beyond the plan's terms.** It is an edge
  we operate, not a CDN.
- **No email for customer domains.** Layer 2 lets customers keep their mail
  records; grund does not host mail.
- **No wildcard custom domains in layer 1.** A wildcard needs the zone
  (layer 2) or DNS-01 delegation.

## Decisions for Kasper

1. The apps domain (recommendation: grundapps.net, with .com alongside).
2. The relay allowance on Homelab, and fair use on paid plans.
3. Layer 2 nameserver hostnames, and build versus buy for authoritative DNS.
4. Whether layer 2 is Pro or Business only (proposed: Pro, with DNSSEC and
   audit log on Business).
