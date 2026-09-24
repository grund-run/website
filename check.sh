#!/usr/bin/env bash
# Verify the site exactly as it ships: the static musl binary built in the CI
# image, packaged by Dockerfile.prebuilt into a read-only scratch container,
# asserted with the same accepttests (tests/accepttest) CI runs.
#
# A script because it is docker orchestration; every assertion lives in the
# Rust accepttests.
set -euo pipefail
cd "$(dirname "$0")"

img=grund-website:check
name=grund-website-check
port=${PORT:-8097}
rust_image=rust:1.98-alpine

cleanup() { docker rm -f "$name" >/dev/null 2>&1 || true; }
trap cleanup EXIT

echo "cargo (host)"
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
echo "  ok   fmt, clippy and tests"

echo "static binary ($rust_image, as CI builds it)"
docker run --rm -v "$PWD":/src -w /src \
  -v grund-website-cargo:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/src/target/musl -e OWNER="$(id -u):$(id -g)" \
  "$rust_image" sh -c 'apk add --no-cache -q musl-dev binutils \
    && cargo build --locked --release; status=$?; chown -R "$OWNER" target/musl; [ $status -eq 0 ] \
    && ! readelf -d target/musl/release/grund-website | grep -q NEEDED'
mkdir -p .image
cp target/musl/release/grund-website .image/grund-website
echo "  ok   statically linked ($(du -h .image/grund-website | cut -f1))"

echo "image"
docker build -q -f Dockerfile.prebuilt -t "$img" .image >/dev/null
echo "  ok   scratch image builds ($(docker images "$img" --format '{{.Size}}'))"

echo "runtime (read-only, as deployed)"
cleanup
docker run -d --name "$name" --read-only -p "127.0.0.1:$port:8080" \
  -e GRUND_WEBSITE_REDIRECT_HOSTS=www.grund.run "$img" >/dev/null
if ! GRUND_WEBSITE_ACCEPT_URL="http://127.0.0.1:$port" GRUND_WEBSITE_ACCEPT_REDIRECT_HOST=www.grund.run \
  cargo test --locked --test tests; then
  docker logs "$name"
  exit 1
fi

echo
echo "all checks passed"
