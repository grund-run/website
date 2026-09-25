---
title: Our gateway broke every site for browsers for 8 hours, and curl said everything was fine
date: 2026-09-25
summary: One routing rule stopped working whenever a browser's first TLS message needed two packets. Every modern browser's does. curl's never does.
draft: true
---

On the evening of 24 September we changed the routing rules on the gateway
that sits in front of every site we run: grund.sh and a dozen others. For
the next eight hours, every one of those sites failed in every browser. Our
checks said they were fine the whole time.

This is what happened, why the checks could not see it, and what now runs
before every gateway change.

## The setup

Our gateway does not decrypt anything. It reads the name the visitor asked
for from the first message of the TLS handshake (the ClientHello's SNI), and
passes the encrypted connection through to the cluster that serves that
name. The certificates live in the clusters.

The change added two private routes: names that should only answer to our
own VPN. The natural way to write that was one rule that matched the name
*and* the client's address.

## What broke

With that rule in place, the gateway stopped matching **any** route, not
just the new ones, whenever a ClientHello arrived in more than one TCP
segment. Connections then fell through to the gateway's own TLS, which has
no certificate for those names, and the browser got a handshake error.

Every modern browser sends a ClientHello that is too big for one segment. It
carries a post-quantum key share (X25519MLKEM768), about 1.2 kB on its own,
which puts the whole hello at around 1.5 kB and over the usual segment size.

curl sends a classic hello of a few hundred bytes, which always fits in one
segment. So every check we ran, including a before-and-after comparison of
30 hosts, passed.

Why that combination of matchers behaves this way inside the layer-4 plugin
we use (caddy-l4 v0.1.2) is **not established**. What is established: the
name alone works, and the name plus the address in one rule breaks
fragmented hellos for every route.

## The fix

Match on the name only. Do the address check in a second step, inside the
route: admit the VPN range, close every other connection. Same behaviour for
the private names, and nothing else can be affected by it.

## What runs now

Two things, and neither was there before:

1. **A pre-apply test.** It builds the gateway exactly as it will run,
   renders the real configuration, and for every route sends three hellos:
   a curl-shaped one, a single-write post-quantum one, and a post-quantum
   one deliberately split into 600-byte segments. Each must reach the right
   backend. Run against the broken rule, it fails 38 of 166 checks. Nothing
   is applied to the gateway without it passing on that exact commit.
2. **A browser-shaped probe every minute.** It connects to every public name
   through the gateway's public address, with both hello shapes, and alerts
   when either fails. It would have caught this in the first minute.

## The lesson

We tested with a tool that is not what our visitors use, and its success was
indistinguishable from real success. "It works with curl" is a statement
about curl.

If you run anything that routes TLS by SNI, test it with a hello that spans
two packets. `openssl s_client -groups X25519MLKEM768` (OpenSSL 3.5 or
newer) sends one, and headless Chrome does too.
