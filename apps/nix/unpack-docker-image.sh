#!/usr/bin/env bash
# Unpack a docker-archive (docker save) into $out with AUFS whiteout handling.
set -euo pipefail

image_tar=$1
out=$2
tarbin=$3
jqbin=$4

apply_whiteouts() {
  find "$rootfs" -name '.wh.*' -print0 | while IFS= read -r -d '' wh; do
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
rootfs="$work/rootfs"
cp "$image_tar" "$work/image.tar"
(
  cd "$work"
  "$tarbin" xf image.tar
  mkdir -p "$rootfs"
  for layer in $("$jqbin" -r '.[0].Layers[]' manifest.json); do
    "$tarbin" --no-same-owner --no-same-permissions -xf "$layer" -C "$rootfs" \
      || "$tarbin" --no-same-owner -xf "$layer" -C "$rootfs"
    apply_whiteouts
  done
)
# Overlay-backed stores can change directory identities during tar extraction.
mkdir -p "$out"
cp -a "$rootfs/." "$out/"
