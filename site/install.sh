#!/bin/sh
# Download the grund CLI; machine setup belongs to `grund install`.
#
#   curl -fsSL https://grund.sh/install | sh
#   curl -fsSL https://grund.sh/install | sh -s -- --version v0.0.0-alpha.3
#
# --domain downloads the CLI, then invokes `grund server install --domain NAME`
# (`grund install --domain NAME` on releases before it).
# Server setup requires Linux, root and systemd. Current releases install
# missing Docker tools from Debian/Ubuntu packages; other systems need
# Docker Engine and Compose v2 first. No sudo is invoked here.
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
grund CLI bootstrap

  --version VERSION   Pin a release, for example v0.0.0-alpha.4
  --channel CHANNEL   stable or alpha (new installs: alpha; existing installs: their channel)
  --bin-dir DIR       Binary directory (default: ~/.local/bin, /usr/local/bin as root)
  --domain NAME       After downloading, install the dashboard here for NAME
  --code CODE         Pass the one-time machine code to that installation
  --no-vms            Pass --no-vms to that installation
  --help              Show this help without changing anything

Without --domain, this downloads only the CLI. With --domain, the downloaded
CLI owns systemd setup and installation. Reinstalling preserves the server
image; changing that image needs an update-capable CLI release and a backup.
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
  curl --proto '=https' --tlsv1.2 --connect-timeout 10 --max-time 300 --max-filesize 268435456 -fsS -o "$work/$file" "$base/$file" || fail "could not download $base/$file"
  [ "$(checksum "$work/$file")" = "$expected" ] || fail "$file failed SHA-256 verification; nothing from this file was installed"
}

main() {
  version= channel= domain= code= bin_dir= no_vms=
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --help|-h) usage; return ;;
      --version|--channel|--domain|--code|--bin-dir)
        [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$1 needs a value"
        case "$1" in
          --version) version=$2 ;;
          --channel) channel=$2 ;;
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
  command -v curl >/dev/null 2>&1 || fail 'curl is required'
  command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || fail 'sha256sum or shasum is required'
  command -v install >/dev/null 2>&1 || fail 'install is required (coreutils)'
  work=$(mktemp -d)
  trap 'rm -rf "$work"; if [ -n "${staged:-}" ]; then rm -f "$staged"; fi' EXIT
  trap 'exit 1' HUP INT TERM
  if [ -z "$channel" ]; then
    channel=alpha
    if [ -x "$bin_dir/grund" ]; then
      installed=$("$bin_dir/grund" --version 2>/dev/null || true)
      case "$installed" in
        "grund v"*)
          installed=${installed#grund }
          if valid_version "$installed"; then
            case "$installed" in *-alpha.*) channel=alpha ;; *) channel=stable ;; esac
          fi ;;
      esac
    fi
  fi
  case "$channel" in stable|alpha) ;; *) fail '--channel is stable or alpha' ;; esac
  if [ -z "$version" ]; then
    : > "$work/candidates"
    page=1
    while :; do
      [ "$page" -le 100 ] || fail 'release list exceeds 100 pages; use --version'
      curl --proto '=https' --tlsv1.2 --connect-timeout 10 --max-time 60 -fsS \
        "https://git.kjuulh.io/api/v1/repos/grund/grund/releases?limit=100&page=$page" \
        -o "$work/releases.json" || fail 'could not resolve releases; use --version'
      tr ',' '\n' < "$work/releases.json" | sed -n 's/^[[:space:]]*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' > "$work/tags"
      [ -s "$work/tags" ] || break
      while IFS= read -r tag; do
        valid_version "$tag" || continue
        case "$channel:$tag" in stable:*-alpha.*) continue ;; esac
        printf '%s\n' "$tag" >> "$work/candidates"
      done < "$work/tags"
      page=$((page + 1))
    done
    # Numeric fields, not publication order or lexical alpha numbering.
    sed -E 's/^v([0-9]+)\.([0-9]+)\.([0-9]+)-alpha\.([0-9]+)$/\1.\2.\3.0.\4 &/; t; s/^v([0-9]+)\.([0-9]+)\.([0-9]+)$/\1.\2.\3.1.0 &/' "$work/candidates" |
      LC_ALL=C sort -t . -k1,1n -k2,2n -k3,3n -k4,4n -k5,5n > "$work/ordered"
    while read -r key tag; do version=$tag; done < "$work/ordered"
    [ -n "$version" ] || fail "no downloadable $channel release is published; nothing was installed"
  fi
  case "$version" in *-alpha.*) printf 'Installing grund %s — alpha software. Back up data; expect changes.\n' "$version" ;; *) printf 'Installing grund %s\n' "$version" ;; esac
  base=https://git.kjuulh.io/api/packages/grund/generic/grund/$version
  curl --proto '=https' --tlsv1.2 --connect-timeout 10 --max-time 60 --max-filesize 1048576 -fsS -o "$work/SHA256SUMS" "$base/SHA256SUMS" || fail "release $version has no published SHA256SUMS"
  artifact=grund-$os-$arch
  fetch "$artifact"
  chmod 0755 "$work/$artifact"
  reported=$("$work/$artifact" --version) || fail 'downloaded executable could not report its version'
  [ "$reported" = "grund $version" ] || fail "downloaded executable does not report grund $version"
  printf '%s\n' "$reported"
  can_install= install_command=
  if "$work/$artifact" server install --help >/dev/null 2>&1; then
    can_install=1 install_command='server install'
  elif "$work/$artifact" install --help >/dev/null 2>&1; then
    can_install=1 install_command=install
  fi
  login='grund login'
  if "$work/$artifact" auth --help >/dev/null 2>&1; then login='grund auth login'; fi
  if [ -n "$domain" ] && [ -z "$can_install" ]; then
    fail "release $version has no grund install command; choose a newer release"
  fi
  mkdir -p "$bin_dir"
  staged=$(mktemp "$bin_dir/.grund.XXXXXXXX")
  if ! install -m 0755 "$work/$artifact" "$staged" || ! mv -f "$staged" "$bin_dir/grund"; then
    rm -f "$staged"
    fail "could not replace $bin_dir/grund"
  fi
  printf 'Installed %s (SHA-256 verified).\n' "$bin_dir/grund"
  case ":${PATH:-}:" in *":$bin_dir:"*) ;; *)
    quoted_directory=$(printf '%s:\n' "$bin_dir" | sed "s/'/'\\\\''/g")
    printf "Add to PATH: export PATH='%s'\"\$PATH\"\n" "$quoted_directory"
    ;; esac
  if [ -n "$domain" ]; then
    set -- $install_command --domain "$domain" --version "$version"
    [ -z "$code" ] || set -- "$@" --code "$code"
    [ -z "$no_vms" ] || set -- "$@" --no-vms
    "$bin_dir/grund" "$@"
  else
    if [ -n "$can_install" ] && [ "$os" = linux ]; then
      quoted_binary=$(printf '%s/grund\n' "$bin_dir" | sed "s/'/'\\\\''/g")
      elevation='sudo '
      [ "$(id -u)" != 0 ] || elevation=
      case "$version" in
        v0.0.0-alpha.[1-9])
          printf "\nNext: %s'%s' %s --domain grund.example.com\n" "$elevation" "$quoted_binary" "$install_command"
          ;;
        *)
          if [ "$version" = v0.0.0-alpha.10 ]; then
            printf '\nOn fresh Debian 13, this old release needs docker.io, docker-cli and docker-compose installed first. Choose alpha.11 or newer for automatic setup.\n'
          fi
          printf "\nNext: %s'%s' %s\n" "$elevation" "$quoted_binary" "$install_command"
          printf 'Local HTTP only; use the SSH tunnel printed by the installer from another computer.\nFor HTTPS: add --domain DNS-NAME.\n'
          ;;
      esac
      printf 'For hosted grund: %s https://app.grund.sh\n' "$login"
    else
      printf '\nNext: %s\nHosted dashboard: https://app.grund.sh\n' "$login"
      if [ -z "$can_install" ]; then
        printf '%s\n' 'This release provides the CLI but not the managed grund install command.'
      else
        printf '%s\n' 'Managed server installation requires Linux.'
      fi
    fi
  fi
}

main "$@"
