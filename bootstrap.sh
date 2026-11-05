#!/bin/sh
# Build portable teka from a source checkout, then initialize the host.
set -eu

status() { printf '  · %s\n' "$*" >&2; }
fail() { printf '  ✗ Bootstrap failed\n    %s\n' "$*" >&2; exit 1; }

[ -n "${HOME:-}" ] || fail 'HOME is not set'

repo_root=$(CDPATH='' cd "$(dirname "$0")" && pwd -P) || fail 'cannot locate repository root'
cd "$repo_root"

os=$(uname -s)
arch=$(uname -m)
case "$os:$arch" in
  Linux:x86_64|Linux:aarch64|Linux:arm64|Darwin:arm64|Darwin:aarch64) ;;
  *) fail "unsupported host $os/$arch; supported: x86_64-linux, aarch64-linux, aarch64-darwin" ;;
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
  status 'Installing Nix with the interactive Determinate installer'
  installer=$(mktemp) || fail 'cannot create installer file'
  trap 'rm -f "$installer"' 0
  curl --proto '=https' --tlsv1.2 -fsSL https://install.determinate.systems/nix -o "$installer" || fail 'cannot download Nix installer'
  sh "$installer" install || fail 'Nix installation failed or was cancelled'
  rm -f "$installer"
  trap - 0
  nix=$(find_nix) || fail 'Nix is unavailable after installation; check the installer output and rerun bootstrap'
fi

status "Checking Nix: $nix"
"$nix" --version >&2 || fail 'Nix is broken; repair the existing installation and rerun bootstrap'
features=$("$nix" eval --json --expr 'builtins ? getFlake') || fail 'Nix evaluation failed; enable nix-command and flakes in your Nix configuration'
[ "$features" = true ] || fail 'enable nix-command and flakes in your Nix configuration'
"$nix" store ping --json >/dev/null || fail 'cannot access the Nix store; check your store setting, daemon and user permissions'

status 'Building portable Teka with Nix'
runtime_root="${XDG_DATA_HOME:-$HOME/.local/share}/teka"
mkdir -p "$(dirname "$runtime_root")" || fail 'cannot create Teka runtime directory'
store_path=$("$nix" build --out-link "$runtime_root" --print-out-paths ./tools/teka#portable) || fail 'Nix build failed'
binary="$store_path/bin/teka"
[ -x "$binary" ] || fail "build did not produce an executable: $binary"
exec "$binary" build init
