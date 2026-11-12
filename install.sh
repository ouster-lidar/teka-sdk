#!/bin/sh
# Download the latest portable Teka release, then initialize the host.
# Pass a release tag to install a specific version: sh install.sh v0.1.0-rc.1
set -eu

fail() {
  printf '✗ %s\n' "$1" >&2
  exit 1
}

main() {
  [ "$#" -le 1 ] || fail 'Usage: install.sh [RELEASE_TAG]'
  tag=${1:-}
  if [ -n "$tag" ]; then
    case "$tag" in
      *[!A-Za-z0-9._-]*) fail 'Install: invalid release tag' ;;
    esac
    case "$tag" in
      v[0-9]*) ;;
      *) fail 'Install: release tag must start with v and a digit' ;;
    esac
  fi
  os=$(uname -s)
  arch=$(uname -m)
  case "$os:$arch" in
    Linux:x86_64) system=x86_64-linux ;;
    Darwin:arm64|Darwin:aarch64) system=aarch64-darwin ;;
    *) fail "Install: unsupported host $os/$arch; supported: x86_64-linux, aarch64-darwin" ;;
  esac

  command -v curl >/dev/null 2>&1 || fail 'Install: curl is required'
  if command -v sha256sum >/dev/null 2>&1; then
    checksum=sha256sum
  elif command -v shasum >/dev/null 2>&1; then
    checksum=shasum
  else
    fail 'Install: sha256sum or shasum is required'
  fi

  work=$(mktemp -d) || fail 'Install: cannot create temporary directory'
  trap 'rm -rf "$work"' 0
  trap 'exit 1' HUP INT TERM
  asset="teka-$system"
  printf '⏳︎ Downloading Teka %s for %s...\n' "${tag:-latest release}" "$system" >&2
  if [ -z "$tag" ]; then
    release_url=$(curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSLI \
      https://github.com/ouster-lidar/teka-sdk/releases/latest \
      -o /dev/null -w '%{url_effective}') || fail 'Install: cannot find the latest published GitHub release'
    case "$release_url" in
      https://github.com/ouster-lidar/teka-sdk/releases/tag/*) ;;
      *) fail 'Install: latest release did not resolve to a release tag' ;;
    esac
    # Pin both downloads to one tag if a new release appears mid-install.
    tag=${release_url##*/}
  fi
  release_url="https://github.com/ouster-lidar/teka-sdk/releases/download/$tag"
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL \
    "$release_url/SHA256SUMS" -o "$work/SHA256SUMS" || fail 'Install: cannot download release checksums; SHA256SUMS is required'
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL \
    "$release_url/$asset" -o "$work/$asset" || fail "Install: cannot download $asset"
  awk -v asset="$asset" '$2 == asset { print; count++ } END { if (count != 1) exit 1 }' \
    "$work/SHA256SUMS" > "$work/checksum" || fail "Install: expected one checksum for $asset"
  (
    cd "$work"
    if [ "$checksum" = shasum ]; then
      shasum -a 256 --check checksum
    else
      sha256sum --check checksum
    fi
  ) || fail 'Install: checksum verification failed'
  chmod +x "$work/$asset"
  printf '✓ Teka: verified; initializing host...\n' >&2

  # curl | sh uses stdin for the script; setup prompts need the terminal instead.
  if ( : < /dev/tty ) 2>/dev/null; then
    "$work/$asset" build init < /dev/tty
  else
    "$work/$asset" build init < /dev/null
  fi
}

main "$@"
