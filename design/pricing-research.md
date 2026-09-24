# Pricing research

Status: research behind the **planned** pricing on grund.run/pricing. grund is
not available; nothing here is a price anyone can pay yet. The plan structure
and license prices below are **Kasper's decision (2026-09-24)**. The grund
machine prices are an Estimate built on provider costs.

Sources: vendor pricing pages, read 2026-09-24 (Observed), and the survey in
`huge-business-plan/tiny/docs/huge/research/selfhosted-paas.md`, read
2026-09-23. Figures are list prices as shown, in the vendor's currency.

## What comparable products charge (Observed, 2026-09-24)

| Product | Who it is for | Model | Price |
|---|---|---|---|
| Coolify, self-hosted | everyone | open source, all features | free forever, no server limit |
| Coolify Cloud | hobby to small business | hosted dashboard per server | $5/mo including 2 servers, +$3 per extra server; 20% off yearly |
| Dokploy Cloud | hobby to small business | per server | Hobby $4.50 per server/mo (1 user); Startup from $15/mo (3 servers), +$4.50 per server; Enterprise custom (SSO, SCIM, audit logs, white label) |
| Portainer | homelab to enterprise | per node | free up to 3 nodes; "Home & Student" tier for non-commercial use; Starter $1,045/yr (5–15 nodes); Scale $2,095/yr (5–25 nodes) |
| Unraid | homelab | one-time licence | Starter $49 (6 devices), Unleashed $109, Lifetime $249; the first two include 1 year of updates |
| Hatchbox | small business | per server | $15 per server/mo, unlimited apps and team members |
| Sidero Omni | technical teams | per node | Hobby $10/mo up to 10 nodes; Startup $25 per node/mo |
| Railway | cloud PaaS, for contrast | usage | Hobby $5/mo; Pro $20/mo; about $20 per vCPU/mo and $10 per GB RAM/mo |

From the 2026-09-23 survey (Observed then): Easypanel is $10.90–37.90 per server
per month, Cloud66 +$12 per extra server, and Coolify Cloud is reported at about
$10k MRR.

## What the market says

- **A free, complete self-hosted version is table stakes.** Coolify gives
  everything away self-hosted and still sells its hosted dashboard. A gated or
  time-limited free tier would read as a trial in this market.
- **Two price bands.** Hobby dashboards cost $3–5 per server. Tools sold to
  businesses cost $12–25 per server or node (Hatchbox $15, Omni $25).
- **Homelabbers pay small amounts, often once.** Unraid's $49–249 one-time
  licences and Portainer's free three nodes set their expectations.
- **Team features separate the business tier.** Dokploy gates SSO, SCIM and
  audit logs to Enterprise; that is the usual line.
- **Yearly discounts are about 20%.** Coolify and Dokploy both.
- **Price by the machine, never by traffic or app count.** Every
  bring-your-own-server competitor does, and it is the point of running on
  your own hardware.

## Decided (Kasper, 2026-09-24)

Running your own machines should be cheap and fair, like Tailscale: it costs
us little. The fee comes from Pro and Business, especially on machines rented
through grund.

| Plan | Your own machines (license) | Account fee |
|---|---|---|
| Homelab | first machine free, then €3 per machine/mo | none |
| Pro | €10 per machine/mo | none |
| Business | €10 per machine/mo | €49/mo |

**grund machines**: servers rented through grund, priced by size. They carry
**no per-machine license**, because the price already includes our fee. They
are available on every plan.

There is no separate "Self-hosted" plan any more; the free Homelab machine is
the free tier. This replaces the earlier proposal (Self-hosted free, Homelab
€5 for 3, Pro €15, Business €49 + €15).

## grund machine prices (Estimate)

The underlying cost is Hetzner, from its price adjustment effective
2026-06-15 (Observed 2026-09-24, docs.hetzner.com). The Cost-Optimized cloud
tier (CX, CAX) shows "Currently not available", so rentable capacity starts at
Regular Performance:

| Hetzner type | €/mo excl. VAT |
|---|---|
| CPX22 / CPX32 / CPX42 / CPX52 | 19.49 / 35.49 / 69.49 / 100.49 |
| CX23 / CX33 / CX43 (unavailable) | 5.49 / 8.49 / 15.99 |
| AX42-1 dedicated (Ryzen 7 PRO 8700GE, 64 GB ECC, 2×512 GB NVMe) | 97.30 |

| grund machine | Offered as | Price | Cost basis |
|---|---|---|---|
| Small | 2 vCPU, 4 GB | €29/mo | CPX22 class, €19.49 |
| Medium | 4 vCPU, 8 GB | €45/mo | CPX32 class, €35.49 |
| Large | 8 vCPU, 16 GB | €79/mo | CPX42 class, €69.49 |
| Dedicated | 8 cores, 64 GB, 2×512 GB NVMe | €109/mo | AX42-1, €97.30 |

The rule is provider cost plus about €10, the same as a Pro license, rounded.
Margins are €9.51 / €9.51 / €9.51 / €11.70. So a grund machine costs what the
same server would cost you rented directly and licensed on Pro: neither a
penalty nor a subsidy for bringing your own. **Not established:** the exact vCPU, RAM and disk of each
CPX type. Hetzner's docs pages fetched did not list them, so the sizes are
what grund offers, and the mapping to CPX types is to be confirmed before
launch. Hetzner has raised prices twice in 2026; these numbers move with it.

## Still open

- The currency shown to non-EU visitors.
- Whether Homelab machines beyond the first get a one-time option.
- Storage and bandwidth limits on grund machines (Hetzner includes traffic
  allowances; to be matched).
