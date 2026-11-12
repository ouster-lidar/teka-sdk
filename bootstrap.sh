#!/bin/sh
# Build portable teka from a source checkout, then initialize the host.
set -eu

timestamp() {
  now=$(date +%s.%N)
  # macOS date lacks fractional seconds; use its bundled Perl instead.
  case "$now" in
    *N*) /usr/bin/perl -MTime::HiRes=time -e 'printf "%.9f\n", time' ;;
    *) printf '%s\n' "$now" ;;
  esac
}

result() {
  printf '%s %s' "$1" "$2" >&2
  if [ -n "$started" ]; then
    awk -v start="$started" -v end="$(timestamp)" 'BEGIN { printf " (%.1fs)", end - start }' >&2
  fi
  printf '\n' >&2
  started=''
}

progress() {
  printf '⏳︎ %s\n' "$1" >&2
  started=$(timestamp)
}

fail() {
  result '✗' "$1"
  if [ "$#" -gt 1 ]; then
    printf '\n→ %s\n' "$2" >&2
  fi
  exit 1
}
started=''

[ -n "${HOME:-}" ] || fail 'Bootstrap: HOME is not set'

repo_root=$(CDPATH='' cd "$(dirname "$0")" && pwd -P) || fail 'Bootstrap: cannot locate repository root'
cd "$repo_root" || fail 'Bootstrap: cannot enter repository root'

os=$(uname -s)
arch=$(uname -m)
case "$os:$arch" in
  Linux:x86_64|Linux:aarch64|Linux:arm64|Darwin:arm64|Darwin:aarch64) ;;
  *) fail "Bootstrap: unsupported host $os/$arch; supported: x86_64-linux, aarch64-linux, aarch64-darwin" ;;
esac

find_nix() {
  if command -v nix >/dev/null 2>&1; then
    command -v nix
  elif [ -e /nix/var/nix/profiles/default/bin/nix ]; then
    printf '%s\n' /nix/var/nix/profiles/default/bin/nix
  else
    return 1
  fi
}

if ! nix=$(find_nix); then
  progress 'Installing Nix (interactive setup)...'
  installer=$(mktemp) || fail 'Nix: cannot create installer file'
  trap 'rm -f "$installer"' 0
  curl --proto '=https' --tlsv1.2 -fsSL https://install.determinate.systems/nix -o "$installer" || fail 'Nix: cannot download installer'
  sh "$installer" install || fail 'Nix: installation failed or was cancelled'
  rm -f "$installer"
  trap - 0
  nix=$(find_nix) || fail 'Nix: unavailable after installation' 'Check the installer output and rerun bootstrap'
fi

version=$("$nix" --version) || fail 'Nix: version check failed' 'Repair the existing Nix installation and rerun bootstrap'
[ -n "$version" ] || fail 'Nix: returned an empty version'
result '✓' "Nix: $version"
features=$("$nix" eval --json --expr 'builtins ? getFlake') || fail 'Nix config: evaluation failed' 'Enable nix-command and flakes in your Nix configuration and rerun bootstrap'
[ "$features" = true ] || fail 'Nix config: flakes unavailable' 'Enable nix-command and flakes in your Nix configuration and rerun bootstrap'
result '✓' 'Nix config: valid (flakes)'
"$nix" store info --json >/dev/null || fail 'Nix store: unreachable' 'Check your store setting, daemon and user permissions'

result '✓' 'Nix store: reachable'
runtime_root="${XDG_DATA_HOME:-$HOME/.local/share}/teka"
mkdir -p "$(dirname "$runtime_root")" || fail 'Portable Teka: cannot create runtime directory'
build_logs=$(mktemp) || fail 'Portable Teka: cannot create build log'
trap 'rm -f "$build_logs"' 0
progress 'Building portable Teka with Nix...'
if ! store_path=$("$nix" build --out-link "$runtime_root" --print-out-paths .#portable 2>"$build_logs"); then
  result '✗' 'Portable Teka: build failed'
  printf '\n' >&2
  cat "$build_logs" >&2
  exit 1
fi
rm -f "$build_logs"
trap - 0
binary="$store_path/bin/teka"
[ -x "$binary" ] || fail 'Portable Teka: build did not produce an executable bin/teka'
result '✓' 'Portable Teka: built'
exec "$binary" build init
