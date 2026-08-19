# Sourced by jsd-* host scripts. Not on PATH.

# apps/flake.nix lives two levels above this file (apps/scripts/lib/).
_JSD_FLAKE_NEAR_LIB=$(
  cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd
)

cd_flake_root() {
  local prefix=${1:-${0##*/}}
  local d

  if [[ -n "${JETSTREAM_FLAKE_ROOT:-}" && -f "${JETSTREAM_FLAKE_ROOT}/flake.nix" ]]; then
    cd "${JETSTREAM_FLAKE_ROOT}"
    return
  fi

  if [[ -f "${_JSD_FLAKE_NEAR_LIB}/flake.nix" ]]; then
    cd "${_JSD_FLAKE_NEAR_LIB}"
    return
  fi

  d=$PWD
  while [[ "$d" != "/" ]]; do
    if [[ -f "$d/flake.nix" ]]; then
      cd "$d"
      return
    fi
    d=$(dirname "$d")
  done

  echo "${prefix}: cannot find flake.nix (set JETSTREAM_FLAKE_ROOT)" >&2
  exit 1
}

resolve_apps_flake() {
  local prefix=${1:-${0##*/}}
  local root="${JETSTREAM_APPS_FLAKE_ROOT:-}"

  if [[ -n "$root" && -f "${root}/flake.nix" ]]; then
    printf '%s\n' "$(cd "${root}" && pwd)"
    return
  fi

  if [[ -f "${_JSD_FLAKE_NEAR_LIB}/flake.nix" ]]; then
    printf '%s\n' "${_JSD_FLAKE_NEAR_LIB}"
    return
  fi

  echo "${prefix}: cannot find apps/flake.nix (set JETSTREAM_APPS_FLAKE_ROOT)" >&2
  exit 1
}
