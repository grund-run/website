# Pricing research

Status: research behind the **planned** pricing on grund.run/pricing. grund is
not available; nothing here is a price anyone can pay yet. Decision for Kasper:
the numbers on the page are a proposal.

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

## The proposal on the page (Estimate)

| Plan | Price | For | Reasoning |
|---|---|---|---|
| Self-hosted | free forever | everyone | Table stakes. Unlimited apps and machines, so the free version is the product, not a trial |
| Homelab | €5/mo, up to 3 machines | homes and hobby projects | In the hobby band (Coolify $5 for 2, Dokploy $4.50 each); a flat price is simpler for a household than per-machine arithmetic |
| Pro | €15 per machine/mo | businesses | Middle of the business band (Hatchbox $15, Omni $25); matches PLATFORM.md's €15 per node |
| Business | €49/mo + €15 per machine | teams that need SSO and an audit log | PLATFORM.md's Business shape; team features are the conventional line |

Yearly billing: two months free (about 17%, close to the market's 20%).

What paid plans add is what we host or do for you: the hosted dashboard,
alerts to phone and email, automatic grund updates, team members, support.
Deploying apps is never gated.

## Not decided

- Whether Homelab is also free for non-commercial use, as Portainer's Home &
  Student is. It costs us little, and it wins the homelab audience's goodwill.
- The currency shown to non-EU visitors.
- A one-time Homelab licence (Unraid-style) instead of monthly.
- Any limit on Pro, such as a minimum of two machines, as PLATFORM.md
  suggested. It is left out for simplicity.
