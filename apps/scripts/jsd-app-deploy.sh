#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=lib/flake-root.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib/flake-root.sh"

log() { echo "jsd-app-deploy: $*" >&2; }
die() { log "$*"; exit 1; }

curl_auth=()
base=""
builder_sys=""
host_sys=""

trap 'rm -f "${tmp:-}" "${log_tmp:-}" "${cookiejar:-}"' EXIT

# Per-app log success patterns (extended grep). Empty = skip success probe.
declare -A APP_LOG_EXPECT=()
APP_LOG_EXPECT+=(['hello-world']='hello world')
APP_LOG_EXPECT+=(['hello-http']='Started hello-http|hello-http.*success')
APP_LOG_EXPECT+=(['hello-webui']='hello-webui')
APP_LOG_EXPECT+=(['hello-wayland-gpu']='hello-wayland-gpu:|vkcube')
APP_LOG_EXPECT+=(['hello-firefox']='hello-firefox')
APP_LOG_EXPECT+=(['hello-firefox-kiosk']='hello-firefox-kiosk')
APP_LOG_EXPECT+=(['hello-ouster-terminal']='hello-ouster-terminal|ouster-cli')
APP_LOG_EXPECT+=(['hello-ouster-world']='hello-ouster-world|PointViz|SimpleViz|open_source|missing OSF')
APP_LOG_EXPECT+=(['hello-pub']='hello-pub listening')
APP_LOG_EXPECT+=(['hello-sub']='hello-sub listening')
APP_LOG_EXPECT+=(['hello-slidev']='hello-slidev listening')
APP_LOG_EXPECT+=(['hello-llm']='hello-llm|Open WebUI|ollama|nemotron')
APP_LOG_EXPECT+=(['nvidia-devtools']='nvidia-devtools listening')

# Known-bad patterns for GPU/Wayland app images.
BAD_PAT='libnvidia-glcore\.so\..*cannot open shared object file|eglGetDisplay failed|Failed at step NAMESPACE|Failed to set up mount namespacing|status=226/NAMESPACE|status=203/EXEC'

usage() {
  echo "usage: jsd-app-deploy help|-h|--help" >&2
  echo "   or: jsd-app-deploy <app-id> [<app-id> ...] [BASE_URL]" >&2
  echo "" >&2
  echo "app ids (from apps flake packages.<system> for this builder):" >&2
  list_targets
  exit 2
}

normalize_machine_arch() {
  case "${1,,}" in
    x86_64|amd64) echo x86_64 ;;
    aarch64|arm64) echo aarch64 ;;
    *) return 1 ;;
  esac
}

arch_to_nix_system() {
  case "$1" in
    x86_64) echo x86_64-linux ;;
    aarch64) echo aarch64-linux ;;
    *) return 1 ;;
  esac
}

mission_control_login() {
  local auth_json auth_disabled authenticated needs_setup enforcement bootstrap_required
  auth_json=$(curl -sS -f "${curl_auth[@]}" "$base/api/v1/auth/status") \
    || die "GET $base/api/v1/auth/status failed"
  auth_disabled=$(printf '%s\n' "$auth_json" | jq -r '.auth_disabled')
  if [[ "$auth_disabled" == "true" ]]; then
    return 0
  fi
  authenticated=$(printf '%s\n' "$auth_json" | jq -r '.authenticated')
  if [[ "$authenticated" == "true" ]]; then
    return 0
  fi
  if [[ -z "${JETSTREAM_MC_PASSWORD:-}" ]]; then
    die "Jetstream requires a session; set JETSTREAM_MC_PASSWORD"
  fi
  cookiejar=$(mktemp)
  curl_auth=(-b "$cookiejar")
  needs_setup=$(printf '%s\n' "$auth_json" | jq -r '.needs_setup')
  if [[ "$needs_setup" == "true" ]]; then
    local setup_body
    bootstrap_required=$(printf '%s\n' "$auth_json" | jq -r '.bootstrap_required')
    if [[ "$bootstrap_required" == "true" ]]; then
      if [[ -z "${JETSTREAM_MC_BOOTSTRAP_TOKEN:-}" ]]; then
        die "Jetstream bootstrap token required; set JETSTREAM_MC_BOOTSTRAP_TOKEN"
      fi
      setup_body=$(
        jq -n --arg p "$JETSTREAM_MC_PASSWORD" --arg t "$JETSTREAM_MC_BOOTSTRAP_TOKEN" \
          '{password: $p, bootstrap_token: $t}'
      )
    else
      setup_body=$(jq -n --arg p "$JETSTREAM_MC_PASSWORD" '{password: $p}')
    fi
    curl -sS -f -c "$cookiejar" -b "$cookiejar" \
      -X POST "$base/api/v1/auth/setup" \
      -H 'Content-Type: application/json' \
      -d "$setup_body" >/dev/null \
      || die "Jetstream setup failed"
    return 0
  fi
  enforcement=$(printf '%s\n' "$auth_json" | jq -r '.enforcement')
  if [[ "$enforcement" == "true" ]]; then
    curl -sS -f -c "$cookiejar" -b "$cookiejar" \
      -X POST "$base/api/v1/auth/login" \
      -H 'Content-Type: application/json' \
      -d "$(jq -n --arg p "$JETSTREAM_MC_PASSWORD" '{password: $p}')" >/dev/null \
      || die "Jetstream login failed (check JETSTREAM_MC_PASSWORD)"
    return 0
  fi
  die "Jetstream requires auth setup before app deploy (POST /api/v1/auth/setup or set JETSTREAM_MC_PASSWORD)"
}

detect_host_arch() {
  local arch
  arch=$(
    curl -sS -f "${curl_auth[@]}" "$base/api/v1/platform/architecture" \
      | jq -r '.architecture // empty'
  ) || return 1
  [[ -n "$arch" ]] || return 1
  normalize_machine_arch "$arch" \
    || {
      log "Jetstream returned unsupported architecture: $arch"
      return 1
    }
}

build_target_for_host() {
  local local_sys=$1
  local app_id=$2
  local target_host_sys=$3

  if [[ "$local_sys" == "$target_host_sys" ]]; then
    printf 'packages.%s.%s\n' "$local_sys" "$app_id"
    return
  fi

  # Non-Linux builders expose short names for the same-CPU Linux target.
  if [[ "$local_sys" == *-darwin && "$target_host_sys" == "${local_sys%-darwin}-linux" ]]; then
    printf 'packages.%s.%s\n' "$local_sys" "$app_id"
    return
  fi

  log "host system $target_host_sys differs from builder $local_sys; using cross target"
  printf 'packages.%s.%s-cross-%s\n' "$local_sys" "$app_id" "$target_host_sys"
}

list_targets() {
  local sys
  sys=$(nix eval --raw --impure --expr builtins.currentSystem)
  nix eval "${apps_flake}#packages.${sys}" --json --apply builtins.attrNames \
    | jq -r '.[] | select(. != "default" and (test("-cross-") | not))' \
    | sort | while read -r n; do log "  $n"; done
}

manager_app_name() {
  case "$1" in
    mc-jetstream-install) echo install ;;
    mc-*) echo "${1#mc-}" ;;
    *) echo "$1" ;;
  esac
}

build_app_image() {
  local id=$1
  local gcroot target
  gcroot="$JETSTREAM_PORTABLE_GCROOT_DIR/${id}-${host_sys}"
  mkdir -p "$JETSTREAM_PORTABLE_GCROOT_DIR"
  if [[ "${E2E_SKIP_BUILD:-0}" == 1 ]]; then
    log "E2E_SKIP_BUILD=1: using existing $gcroot (host system ${host_sys})"
    return 0
  fi
  target=$(build_target_for_host "$builder_sys" "$id" "$host_sys")
  log "building ${apps_flake}#${target} -> $gcroot"
  if command -v nom >/dev/null 2>&1; then
    if ! nom build "${apps_flake}#${target}" -L --out-link "$gcroot"; then
      die "nix build failed for $id (target ${apps_flake}#${target})"
    fi
  elif ! nix build "${apps_flake}#${target}" -L --out-link "$gcroot"; then
    die "nix build failed for $id (target ${apps_flake}#${target})"
  fi
}

upload_app_image() {
  local id=$1
  local gcroot raw raw_bytes app_name deploy_url http_code log_expect
  local -a raws
  gcroot="$JETSTREAM_PORTABLE_GCROOT_DIR/${id}-${host_sys}"
  # Per-package $out has exactly one DDI. Do not nix-eval the compose name here:
  # E2E_SKIP_BUILD can leave a gcroot from a different flake rev.
  shopt -s nullglob
  raws=( "$gcroot"/*.raw )
  shopt -u nullglob
  (( ${#raws[@]} == 1 )) || die "expected exactly one .raw in $gcroot (got ${#raws[@]})"
  raw=$(readlink -f "${raws[0]}")
  [[ -f "$raw" ]] || die "missing $raw"
  raw_bytes=$(stat -c '%s' "$raw" 2>/dev/null || wc -c <"$raw")
  log "raw=$raw ($(numfmt --to=iec-i --suffix=B "$raw_bytes" 2>/dev/null || echo "${raw_bytes}B"))"

  app_name=$(manager_app_name "$id")

  curl -sS -f "${curl_auth[@]}" \
    -X POST "$base/api/v1/manager/app/$(printf '%s' "$app_name" | jq -sRr @uri)/remove" \
    >/dev/null 2>&1 || true
  if [[ "${JETSTREAM_DEPLOY_REMOVE_SLEEP:-1}" != 0 ]]; then
    sleep "${JETSTREAM_DEPLOY_REMOVE_SLEEP:-1}"
  fi

  deploy_url="$base/api/v1/manager/install"
  if [[ -n "$JETSTREAM_PORTABLE_PROFILE" && "$JETSTREAM_PORTABLE_PROFILE" != "default" ]]; then
    deploy_url+="?profile=$(printf '%s' "$JETSTREAM_PORTABLE_PROFILE" | jq -sRr @uri)"
  else
    log "profile omitted"
  fi
  tmp=$(mktemp)
  # Stream from disk; --data-binary loads the whole .raw into RAM (fails on large images).
  http_code=$(
    curl -sS -w '%{http_code}' -o "$tmp" "${curl_auth[@]}" \
      -X POST -H 'Content-Type: application/octet-stream' \
      --upload-file "$raw" \
      "$deploy_url"
  ) || die "curl failed uploading app image"
  if [[ "$http_code" != "200" ]]; then
    log "deploy failed HTTP $http_code (profile: ${JETSTREAM_PORTABLE_PROFILE:-<default>})"
    if command -v sed >/dev/null 2>&1; then
      sed -n '1,200p' "$tmp" >&2 || true
    else
      head -n 200 "$tmp" >&2 || true
    fi
    exit 1
  fi
  log "deploy ok (HTTP 200)"
  sleep "$E2E_POST_DEPLOY_SEC"

  log_tmp=$(mktemp)
  logs_url="$base/api/v1/manager/app/$(printf '%s' "$app_name" | jq -Rr @uri)/logs?since_unit_start=true"
  timeout "$E2E_LOG_SEC" curl -sS -N "${curl_auth[@]}" "$logs_url" >"$log_tmp" 2>/dev/null || true
  if command -v sed >/dev/null 2>&1; then
    sed -n '1,200p' "$log_tmp" >&2
  else
    head -n 200 "$log_tmp" >&2 || true
  fi

  grep -qF "Invalid portable image reference" "$log_tmp" 2>/dev/null && die "FAIL: logs API rejected image="
  grep -Eq "$BAD_PAT" "$log_tmp" 2>/dev/null && die "FAIL: matched known-bad GL/X/NAMESPACE pattern"

  log_expect=${APP_LOG_EXPECT["$id"]:-}
  if [[ -n "$log_expect" ]]; then
    grep -Eq "$log_expect" "$log_tmp" 2>/dev/null \
      && log "OK: saw expected log lines for $id" \
      || log "WARN: no lines matching log expectation for $id"
  else
    log "OK: deploy complete (no log expectation for $id)"
  fi
}

deploy_one_app() {
  build_app_image "$1"
  upload_app_image "$1"
}

cd_flake_root jsd-app-deploy
apps_flake=$(resolve_apps_flake jsd-app-deploy)

case "${1:-}" in
  help|-h|--help) [[ $# -eq 1 ]] || usage; list_targets; exit 0 ;;
esac
[[ $# -ge 1 ]] || usage

declare -a ids=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    http://*|https://*)
      base=$1
      shift
      ;;
    *)
      ids+=("$1")
      shift
      ;;
  esac
done
[[ ${#ids[@]} -gt 0 ]] || usage
base=${base:-http://127.0.0.1:80}
base=${base%/}

if [[ "${VM_FAST_DEPLOY:-0}" == 1 ]]; then
  : "${E2E_LOG_SEC:=2}"
  : "${E2E_POST_DEPLOY_SEC:=1}"
  : "${JETSTREAM_DEPLOY_REMOVE_SLEEP:=0}"
else
  : "${E2E_LOG_SEC:=10}"
  : "${E2E_POST_DEPLOY_SEC:=${E2E_POST_APPLY_SEC:-3}}"
  : "${JETSTREAM_DEPLOY_REMOVE_SLEEP:=1}"
fi
: "${JETSTREAM_PORTABLE_PROFILE:=trusted}"
: "${JETSTREAM_PORTABLE_GCROOT_DIR:=tmp/gcroots}"

mission_control_login

builder_sys=$(nix eval --raw --impure --expr builtins.currentSystem)
host_arch=$(detect_host_arch) \
  || die "failed to read valid host architecture from $base/api/v1/platform/architecture"
host_sys=$(arch_to_nix_system "$host_arch") \
  || die "unsupported host architecture from Jetstream: $host_arch"

if [[ ${#ids[@]} -gt 1 && "${E2E_SKIP_BUILD:-0}" != 1 ]]; then
  pids=()
  for id in "${ids[@]}"; do
    build_app_image "$id" &
    pids+=($!)
  done
  for pid in "${pids[@]}"; do
    wait "$pid" || die "parallel nix build failed"
  done
  for id in "${ids[@]}"; do
    upload_app_image "$id"
  done
else
  for id in "${ids[@]}"; do
    deploy_one_app "$id"
  done
fi
