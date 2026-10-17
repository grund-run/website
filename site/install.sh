#!/bin/sh
# Install the grund alpha CLI. Read this script before running it.
#
#   curl -fsSL https://grund.sh/install | sh
#   curl -fsSL https://grund.sh/install | sh -s -- --version v0.0.0-alpha.3
#
# --domain grund.example.com installs the self-hosted dashboard on this Linux
# machine, not an app from the current directory. It requires root, systemd
# and Docker Compose v2. Point the name at this machine and open TCP 443 first.
# Create the owner using the printed command, then obtain a machine code from
# its Machines page and rerun with --domain NAME --code CODE to connect it.
#
# Downloads use the public git.kjuulh.io registry and its SHA256SUMS over HTTPS.
# Checksums detect corruption, not a compromised registry; releases are not
# signed. Existing binaries are replaced only after verification. No sudo is
# invoked, and existing instance secrets, data and machine identity are kept.
# main is called last, so a truncated download cannot start an installation.

set -eu

fail() { printf 'grund install: %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'HELP'
grund alpha installer

  --version VERSION   Pin a release, for example v0.0.0-alpha.3 (default: newest release, including alphas)
  --bin-dir DIR       Binary directory (default: ~/.local/bin, /usr/local/bin as root)
  --domain NAME       Self-host the dashboard with HTTPS on this Linux machine
  --code CODE         Connect this machine after creating the instance owner
  --no-vms            Do not install Firecracker for the local machine
  --help              Show this help without changing anything

Self-hosting requires root, systemd and Docker Compose v2. It stores its
configuration in /opt/grund and persistent data in Docker volumes. It does not
configure DNS, mail, backups or deploy an app. No sudo is invoked for you.
HELP
}

valid_version() {
  printf '%s\n' "$1" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-alpha\.[1-9][0-9]*)?$'
}

checksum() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

fetch() {
  file=$1
  expected=
  while read -r hash name extra; do
    if [ "$name" = "$file" ] && [ -z "$extra" ]; then
      [ -z "$expected" ] || fail "release $version repeats the checksum for $file"
      expected=$hash
    fi
  done < "$work/SHA256SUMS"
  [ "${#expected}" -eq 64 ] || fail "release $version has no unambiguous checksum for $file"
  case "$expected" in *[!0-9a-f]*) fail "invalid checksum for $file" ;; esac
  curl --proto '=https' --tlsv1.2 -fsSL -o "$work/$file" "$base/$file" || fail "could not download $base/$file"
  [ "$(checksum "$work/$file")" = "$expected" ] || fail "$file failed SHA-256 verification; nothing from this file was installed"
}

self_host() {
  root=/opt/grund
  mkdir -p "$root"
  if [ -e "$root/.env" ]; then
    [ -f "$root/.installer-domain" ] || fail "$root/.env is not managed by this installer; keep using your existing deployment"
    [ "$(cat "$root/.installer-domain")" = "$domain" ] || fail "this machine is configured for another domain; edit $root/.env deliberately instead"
    grep -q '^GRUND_IMAGE=' "$root/.env" || fail "$root/.env no longer has the managed GRUND_IMAGE setting"
    sed "s|^GRUND_IMAGE=.*|GRUND_IMAGE=$image|" "$root/.env" > "$work/.env"
  else
    umask 077
    printf 'GRUND_DOMAIN=%s\nGRUND_IMAGE=%s\nGRUND_PULL_POLICY=missing\n' "$domain" "$image" > "$work/.env"
  fi
  install -m 0600 "$work/.env" "$root/.env"
  printf '%s\n' "$domain" > "$root/.installer-domain"
  install -m 0644 "$work/compose.yaml" "$root/compose.yaml"
  install -m 0755 "$work/agent-install.sh" "$root/agent-install.sh"
  printf '%s\n' "Starting grund $version at https://$domain (TCP 443 must reach this machine)."
  docker compose --project-directory "$root" up --no-build -d --wait --wait-timeout 180 || fail "server did not become healthy; inspect: docker compose --project-directory $root logs"
  set -- --url "https://$domain" --binary "$bin_dir/grund"
  [ -z "$no_vms" ] || set -- "$@" --no-vms
  if [ -n "$code" ]; then
    set -- "$@" --code "$code"
  else
    set -- "$@" --prepare-only
  fi
  sh "$root/agent-install.sh" "$@"
  printf '\nSelf-hosted dashboard: https://%s\n' "$domain"
  if [ -z "$code" ] && [ ! -f /var/lib/grund/agent/machine.json ]; then
    printf '%s\n' 'Create the owner with the one-time link printed by:'
    printf '  docker compose --project-directory %s exec grund /grund setup-link\n' "$root"
    printf '%s\n' "Then use its Machines page to obtain a code and connect this machine:"
    printf '  curl -fsSL https://grund.sh/install | sh -s -- --version %s --domain %s --code CODE\n' "$version" "$domain"
    printf '%s\n' 'The local machine is installed but not connected until that step succeeds.'
  fi
  printf 'Sign in to this instance: grund login https://%s\n' "$domain"
  printf '%s\n' 'Configure mail and back up both the grund-data and grund-postgres volumes before depending on it.'
}

main() {
  version= domain= code= bin_dir= no_vms=
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --help|-h) usage; return ;;
      --version|--domain|--code|--bin-dir)
        [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$1 needs a value"
        case "$1" in
          --version) version=$2 ;;
          --domain) domain=$2 ;;
          --code) code=$2 ;;
          --bin-dir) bin_dir=$2 ;;
        esac
        shift 2 ;;
      --version=*) version=${1#*=}; [ -n "$version" ] || fail '--version needs a value'; shift ;;
      --domain=*) domain=${1#*=}; [ -n "$domain" ] || fail '--domain needs a value'; shift ;;
      --no-vms) no_vms=1; shift ;;
      *) fail "unknown argument: $1 (use --help)" ;;
    esac
  done
  [ -z "$code" ] || [ -n "$domain" ] || fail '--code requires --domain'
  [ -z "$version" ] || valid_version "$version" || fail "invalid release version: $version"
  case "$(uname -s)" in Linux) os=linux ;; Darwin) os=darwin ;; *) fail "unsupported OS: $(uname -s)" ;; esac
  case "$(uname -m)" in x86_64|amd64) arch=amd64 ;; aarch64|arm64) arch=arm64 ;; *) fail "unsupported architecture: $(uname -m)" ;; esac
  if [ -z "$bin_dir" ]; then
    if [ "$(id -u)" = 0 ]; then bin_dir=/usr/local/bin
    else [ -n "${HOME:-}" ] || fail 'HOME is not set'; bin_dir=$HOME/.local/bin; fi
  fi
  case "$bin_dir" in /*) ;; *) fail '--bin-dir must be an absolute path' ;; esac
  if [ -n "$domain" ]; then
    [ "$os" = linux ] && [ "$(id -u)" = 0 ] || fail '--domain requires Linux and root; run the reviewed script as root'
    [ "$bin_dir" = /usr/local/bin ] || fail '--domain installs the service binary in /usr/local/bin'
    printf '%s\n' "$domain" | grep -Eq '^([a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?\.)+[a-zA-Z]([a-zA-Z0-9-]*[a-zA-Z0-9])?$' || fail '--domain takes a DNS name, without https://, a port or a path'
    command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ] || fail '--domain needs a running systemd system'
    docker compose version >/dev/null 2>&1 || fail '--domain requires Docker Engine and Docker Compose v2; install them first'
    docker info >/dev/null 2>&1 || fail 'Docker Engine is not available to this user'
  fi
  command -v curl >/dev/null 2>&1 || fail 'curl is required'
  command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || fail 'sha256sum or shasum is required'
  command -v install >/dev/null 2>&1 || fail 'install is required (coreutils)'
  work=$(mktemp -d)
  trap 'rm -rf "$work"; if [ -n "${staged:-}" ]; then rm -f "$staged"; fi' EXIT
  trap 'exit 1' HUP INT TERM
  if [ -z "$version" ]; then
    # Gitea's /releases/latest ignores prereleases. Public list order is newest
    # first; choose its first supported tag, including alpha releases.
    curl --proto '=https' --tlsv1.2 -fsSL 'https://git.kjuulh.io/api/v1/repos/grund/grund/releases?limit=100' -o "$work/releases.json" || fail 'could not resolve the newest release; use --version to pin one'
    tr ',' '\n' < "$work/releases.json" | sed -n 's/^[[:space:]]*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' > "$work/tags"
    while IFS= read -r tag; do
      if valid_version "$tag"; then version=$tag; break; fi
    done < "$work/tags"
    [ -n "$version" ] || fail 'no downloadable grund release is published yet; nothing was installed'
  fi
  case "$version" in *-alpha.*) printf 'Installing grund %s — alpha software. Back up data; expect changes.\n' "$version" ;; *) printf 'Installing grund %s\n' "$version" ;; esac
  base=https://git.kjuulh.io/api/packages/grund/generic/grund/$version
  curl --proto '=https' --tlsv1.2 -fsSL -o "$work/SHA256SUMS" "$base/SHA256SUMS" || fail "release $version has no published SHA256SUMS"
  artifact=grund-$os-$arch
  fetch "$artifact"
  chmod 0755 "$work/$artifact"
  "$work/$artifact" --version
  if [ -n "$domain" ]; then
    fetch compose.yaml
    fetch agent-install.sh
    fetch image-digest.txt
    image=$(cat "$work/image-digest.txt")
    printf '%s\n' "$image" | grep -Eq '^git\.kjuulh\.io/grund/grund@sha256:[0-9a-f]{64}$' || fail 'release names no valid public image digest'
  fi
  mkdir -p "$bin_dir"
  staged=$(mktemp "$bin_dir/.grund.XXXXXXXX")
  if ! install -m 0755 "$work/$artifact" "$staged" || ! mv -f "$staged" "$bin_dir/grund"; then
    rm -f "$staged"
    fail "could not replace $bin_dir/grund"
  fi
  printf 'Installed %s (SHA-256 verified).\n' "$bin_dir/grund"
  case ":${PATH:-}:" in *":$bin_dir:"*) ;; *) printf 'Add to PATH: export PATH="%s:$PATH"\n' "$bin_dir" ;; esac
  if [ -n "$domain" ]; then self_host
  else printf '\nNext: grund login\nHosted dashboard: https://app.grund.sh\n'; fi
}

main "$@"
