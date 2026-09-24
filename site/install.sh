#!/bin/sh
# grund installer
#
#   curl -fsSL https://grund.run/install.sh | sh -s -- --domain app.example.com
#
# One command, run on the machine that will host your app, from the app's
# directory. When grund is released it will install grund on this machine and
# put the app in this directory live at the domain you give it.
#
# grund is in development. Until it is released this script only says so and
# exits. It downloads nothing, writes nothing and changes nothing on this
# machine. Read it first, as you should with any script you pipe into sh.
#
# Everything runs inside main(), called on the last line, so a download cut
# short can never run half a script.

set -eu

main() {
  domain=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --domain)
        if [ "$#" -ge 2 ]; then
          domain=$2
          shift 2
        else
          shift
        fi
        ;;
      --domain=*)
        domain=${1#--domain=}
        shift
        ;;
      *)
        shift
        ;;
    esac
  done

  printf '%s\n' "grund is not available yet."
  printf '\n'
  if [ -n "$domain" ]; then
    printf 'When it is, this command will install grund on this machine and put\n'
    printf 'the app in this directory live at https://%s.\n' "$domain"
  else
    printf 'When it is, this command will install grund on this machine, ready\n'
    printf 'to deploy your apps.\n'
  fi
  printf '\n'
  printf '%s\n' "Nothing was changed on this machine."
  printf '%s\n' "Follow the build: https://github.com/grund-run"
  exit 1
}

main "$@"
