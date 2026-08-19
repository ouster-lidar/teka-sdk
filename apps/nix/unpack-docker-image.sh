#!/usr/bin/env bash
# Unpack a docker-archive (docker save) into $out with AUFS whiteout handling.
set -euo pipefail

image_tar=$1
out=$2
tarbin=$3
jqbin=$4

apply_whiteouts() {
  find "$out" -name '.wh.*' -print0 | while IFS= read -r -d '' wh; do
    dir=$(dirname "$wh")
    base=$(basename "$wh")
    case "$base" in
      .wh..wh..opq) rm -f "$wh" ;;
      .wh.*)
        target="${base#.wh.}"
        rm -rf "$dir/$target" "$wh"
        ;;
    esac
  done
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp "$image_tar" "$work/image.tar"
(
  cd "$work"
  "$tarbin" xf image.tar
  mkdir -p "$out"
  for layer in $("$jqbin" -r '.[0].Layers[]' manifest.json); do
    "$tarbin" --no-same-owner --no-same-permissions -xf "$layer" -C "$out" \
      || "$tarbin" --no-same-owner -xf "$layer" -C "$out"
    apply_whiteouts
  done
)
