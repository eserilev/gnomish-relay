#!/bin/sh
# Installs gnomish-relay from the latest GitHub Release, and runs setup (SPEC.md 11.3).
#   curl -fsSL https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.sh | sh
# Arguments go to setup (`| sh -s -- --roots ~/code`). With none, setup gets --autostart.
# GNOMISH_URL changes the download folder, and GNOMISH_BIN the install folder.
set -eu

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
  Darwin-arm64) target=aarch64-apple-darwin ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  *)
    echo "error: there is no build for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

url=${GNOMISH_URL:-https://github.com/eserilev/gnomish-relay/releases/latest/download}
bin=${GNOMISH_BIN:-$HOME/.local/bin}
name=gnomish-relay-$target.tar.gz
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -fsSL "$url/$name" -o "$tmp/$name"
curl -fsSL "$url/$name.sha256" -o "$tmp/$name.sha256"
cd "$tmp"
if command -v sha256sum > /dev/null; then
  sha256sum -c "$name.sha256" > /dev/null
else
  shasum -a 256 -c "$name.sha256" > /dev/null
fi
tar -xzf "$name"
mkdir -p "$bin"
install -m 0755 gnomish-relay "$bin/gnomish-relay"
echo "installed $bin/gnomish-relay"

if [ $# -eq 0 ]; then
  set -- --autostart
fi
# `curl | sh` gives the script to sh on stdin, so setup asks its questions on the terminal.
if (: < /dev/tty) 2> /dev/null; then
  "$bin/gnomish-relay" setup "$@" < /dev/tty
else
  "$bin/gnomish-relay" setup "$@"
fi
