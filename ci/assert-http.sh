#!/usr/bin/env sh
# The HTTP contract of the grund.run server, asserted against a running
# instance. One file, three audiences: CI runs it against the release binary,
# check.sh against the scratch image, and a human against a live origin, so all
# three agree by construction.
#
#   ci/assert-http.sh <base-url> [redirect-host] [canonical-origin]
#
#   ci/assert-http.sh http://127.0.0.1:8080 www.grund.run https://grund.run
#   ci/assert-http.sh https://grund.run www.grund.run
#   ci/assert-http.sh https://dev.grund.run
#
# With an http:// base the redirect host is sent as a Host header to the same
# address; with https:// it is requested by name, which also proves its DNS,
# gateway route and certificate. Set EXPECT_NOINDEX=1 for dev.
#
# It asserts behaviour, not copy: nothing here depends on what the designed
# site says, only on how it is served.
set -eu

base="${1:?usage: assert-http.sh <base-url> [redirect-host] [canonical-origin]}"
base="${base%/}"
redirect_host="${2:-}"
canonical="${3:-https://grund.run}"

fails=0
checks=0
ok()   { checks=$((checks+1)); printf '  ok   %s\n' "$1"; }
bad()  { checks=$((checks+1)); fails=$((fails+1)); printf '  FAIL %s\n' "$1"; }
want() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1 (got '$2', wanted '$3')"; fi; }
has()  { if printf '%s' "$3" | grep -qi -- "$2"; then ok "$1"; else bad "$1 (no '$2')"; fi; }
lacks() { if printf '%s' "$3" | grep -qi -- "$2"; then bad "$1 (found '$2')"; else ok "$1"; fi; }
# Header value, case-insensitive name, CR stripped.
hval() { printf '%s' "$1" | tr -d '\r' | grep -i "^$2:" | head -n1 | cut -d' ' -f2-; }

# --path-as-is keeps curl from normalising the traversal probes away before
# they reach the server.
code() { curl -s --path-as-is -o /dev/null -w '%{http_code}' "$@"; }
heads() { curl -s --path-as-is -D - -o /dev/null "$@"; }

count=0
until curl -sf -o /dev/null "$base/health/live"; do
  count=$((count+1))
  if [ "$count" -ge 80 ]; then echo "  FAIL $base/health/live never answered"; exit 1; fi
  sleep 0.25
done

echo "health"
want "/health/live is 200"  "$(code "$base/health/live")"  200
want "/health/ready is 200" "$(code "$base/health/ready")" 200
live=$(heads "$base/health/live")
has "health answers JSON" "content-type: application/json" "$live"
has "health is never cached" "cache-control: no-store" "$live"
ready=$(curl -s "$base/health/ready")
has "readiness reports the build revision" '"revision":"' "$ready"
has "readiness reports the site digest" '"site_digest":"' "$ready"

echo "pages"
root=$(heads "$base/")
want "/ is 200" "$(code "$base/")" 200
has "/ is HTML" "content-type: text/html; charset=utf-8" "$root"
has "/ revalidates on every use" "cache-control: public, max-age=0, must-revalidate" "$root"
has "/ carries a strong ETag" 'etag: "' "$root"
etag=$(hval "$root" etag)
want "a matching If-None-Match is 304" "$(code -H "If-None-Match: $etag" "$base/")" 304

body_len=$(curl -s "$base/" | wc -c | tr -d ' ')
head=$(curl -s -I "$base/")
want "HEAD / is 200" "$(printf '%s' "$head" | head -n1 | cut -d' ' -f2)" 200
want "HEAD / reports the GET Content-Length" "$(hval "$head" content-length)" "$body_len"
want "HEAD / sends no body" "$(curl -s -I "$base/" -o /dev/null -w '%{size_download}')" 0

br=$(heads -H 'Accept-Encoding: br, gzip' "$base/")
has "a brotli-capable client gets brotli" "content-encoding: br" "$br"
has "compressed responses vary on Accept-Encoding" "vary: accept-encoding" "$br"
gz=$(heads -H 'Accept-Encoding: gzip' "$base/")
has "a gzip-only client gets gzip" "content-encoding: gzip" "$gz"
lacks "a client without Accept-Encoding gets identity" "content-encoding" "$root"

asset=$(curl -s "$base/" | grep -o '/assets/[A-Za-z0-9._~@+-]*' | head -n1 || true)
if [ -n "$asset" ]; then
  want "$asset is 200" "$(code "$base$asset")" 200
  has "hashed assets are immutable" "cache-control: public, max-age=31536000, immutable" "$(heads "$base$asset")"
else
  bad "the home page links at least one /assets/ file"
fi

echo "not found and refusals"
missing=$(heads "$base/definitely/not/a/page")
want "an unknown path is 404" "$(code "$base/definitely/not/a/page")" 404
has "the 404 is the HTML document" "content-type: text/html" "$missing"
has "the 404 is not cached as a page" "cache-control: no-cache" "$missing"
for probe in '/../Cargo.toml' '/%2e%2e/%2e%2e/etc/passwd' '/assets/..%2f..%2fCargo.toml' '/assets/..%5c..%5cCargo.toml' '/.git/config' '//etc/passwd'; do
  want "traversal $probe is 404" "$(code "$base$probe")" 404
done
want "POST is 405" "$(code -X POST "$base/")" 405
has "405 names the allowed methods" "allow: GET, HEAD" "$(heads -X POST "$base/")"

echo "security headers (on a page, a 404 and a refusal)"
for response in "$root" "$missing" "$(heads -X POST "$base/")"; do
  csp=$(hval "$response" content-security-policy)
  has "CSP starts from default-src 'none'" "default-src 'none'" "$csp"
  has "CSP allows scripts only from self" "script-src 'self'" "$csp"
  lacks "CSP has no unsafe-inline" "unsafe-inline" "$csp"
  lacks "CSP has no unsafe-eval" "unsafe-eval" "$csp"
  has "CSP forbids framing" "frame-ancestors 'none'" "$csp"
  has "CSP pins the base URI" "base-uri 'none'" "$csp"
  has "nosniff" "x-content-type-options: nosniff" "$response"
  has "referrer policy" "referrer-policy: strict-origin-when-cross-origin" "$response"
  has "frame denial for old browsers" "x-frame-options: DENY" "$response"
done
if [ "${EXPECT_NOINDEX:-0}" = 1 ]; then
  has "this host asks not to be indexed" "x-robots-tag: noindex" "$root"
else
  lacks "this host may be indexed" "x-robots-tag" "$root"
fi

if [ -n "$redirect_host" ]; then
  echo "canonical host"
  case "$base" in
    https://*) alias() { heads "https://$redirect_host$1"; } ;;
    *)         alias() { heads -H "Host: $redirect_host" "$base$1"; } ;;
  esac
  moved=$(alias '/some/path?ref=x')
  want "$redirect_host redirects with 308" "$(printf '%s' "$moved" | head -n1 | cut -d' ' -f2)" 308
  want "the redirect keeps path and query" "$(hval "$moved" location)" "$canonical/some/path?ref=x"
  has "the redirect carries the CSP" "content-security-policy" "$moved"
fi

echo
if [ "$fails" -eq 0 ]; then
  echo "all $checks checks passed against $base"
else
  echo "$fails of $checks checks FAILED against $base"
  exit 1
fi
