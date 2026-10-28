#!/bin/sh
# Build and install teka for the current host.
# For teka on PATH without installing, run: nix develop (repository root).
set -eu

status() { printf '[*] %s\n' "$*"; }
fail() { printf '[-] %s\n' "$*" >&2; exit 1; }

[ -n "${HOME:-}" ] || fail 'HOME is not set'

repo_root=$(CDPATH='' cd "$(dirname "$0")" && pwd -P) || fail 'cannot locate repository root'
cd "$repo_root"

os=$(uname -s)
arch=$(uname -m)
case "$os:$arch" in
  Linux:x86_64|Linux:aarch64|Linux:arm64|Darwin:arm64|Darwin:aarch64) ;;
  *) fail "unsupported host $os/$arch; supported: x86_64-linux, aarch64-linux, aarch64-darwin" ;;
esac

if command -v nix >/dev/null 2>&1; then
  builder=nix
elif command -v cargo >/dev/null 2>&1; then
  builder=cargo
else
  fail 'neither Nix nor Cargo is installed; prebuilt binary releases will be available in the future'
fi

local_bin="$HOME/.local/bin"
case ":${PATH-}:" in
  *":$local_bin:"*|*":$local_bin/:"*)
    install_dir=$local_bin
    needs_sudo=no
    if [ -d "$install_dir" ]; then
      [ -w "$install_dir" ] || fail "cannot write to $install_dir"
    else
      parent="$HOME"
      [ ! -d "$HOME/.local" ] || parent="$HOME/.local"
      [ -w "$parent" ] || fail "cannot create $install_dir"
    fi
    ;;
  *)
    install_dir=/usr/local/bin
    if [ -d "$install_dir" ]; then
      [ -w "$install_dir" ] && needs_sudo=no || needs_sudo=yes
    elif [ -d /usr/local ]; then
      [ -w /usr/local ] && needs_sudo=no || needs_sudo=yes
    else
      needs_sudo=yes
    fi
    ;;
esac

destination="$install_dir/teka"
[ ! -d "$destination" ] || fail "destination is a directory: $destination"
if [ "$needs_sudo" = yes ]; then
  command -v sudo >/dev/null 2>&1 || fail "cannot write to $install_dir and sudo is unavailable"
fi

if [ "$needs_sudo" = yes ]; then
  status "Install: $destination (sudo required)"
  printf '[*] Continue with build and install? [y/N] '
  IFS= read -r answer || fail 'confirmation required for system install'
  case "$answer" in
    y|Y|yes|YES|Yes) ;;
    *) status 'Cancelled'; exit 0 ;;
  esac
else
  status "Install: $destination"
fi

run_install_command() {
  if [ "$needs_sudo" = yes ]; then
    sudo "$@"
  else
    "$@"
  fi
}

status "Building with $builder"
if [ "$builder" = nix ]; then
  runtime_root="${XDG_DATA_HOME:-$HOME/.local/share}/teka"
  mkdir -p "$(dirname "$runtime_root")" || fail 'cannot create Teka runtime directory'
  store_path=$(nix build --out-link "$runtime_root" --print-out-paths ./tools/teka#portable) || fail 'Nix build failed'
  binary="$store_path/bin/teka"
else
  case "$os" in
    Linux) verifier=sbverify; dependency='sbsigntool (Linux package manager)' ;;
    Darwin) verifier=osslsigncode; dependency='osslsigncode (brew install osslsigncode)' ;;
  esac
  command -v "$verifier" >/dev/null 2>&1 || fail "$verifier is required for package verification; install $dependency"
  command -v rustc >/dev/null 2>&1 || fail 'rustc is required when building with Cargo'
  cargo_target=$(rustc -vV | sed -n 's/^host: //p')
  [ -n "$cargo_target" ] || fail 'cannot determine Rust host target'
  target_dir=${CARGO_TARGET_DIR:-"$repo_root/tools/teka/target"}
  case "$target_dir" in
    /*) ;;
    *) target_dir="$repo_root/$target_dir" ;;
  esac
  cargo build --release --locked --manifest-path "$repo_root/tools/teka/Cargo.toml" \
    --package ouster-teka --target "$cargo_target" --target-dir "$target_dir" || fail 'Cargo build failed'
  binary="$target_dir/$cargo_target/release/teka"
fi

[ -x "$binary" ] || fail "build did not produce an executable: $binary"

if [ "$needs_sudo" = yes ]; then
  sudo -v || fail 'sudo authorization failed'
fi

status "Installing to $destination"
run_install_command mkdir -p "$install_dir" || fail "cannot create $install_dir"
temporary=$(run_install_command mktemp "$install_dir/.teka.XXXXXX") || fail "cannot create a temporary file in $install_dir"
trap 'run_install_command rm -f "$temporary"' 0
run_install_command install -m 0755 "$binary" "$temporary" || fail "cannot install to $destination"
run_install_command mv -f "$temporary" "$destination" || fail "cannot replace $destination"
trap - 0

printf '[+] Installed %s\n' "$destination"
