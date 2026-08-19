# Canonical mkfs.erofs flags for portable app DDIs.
#
# Same tuning as the appliance rootfs (lz4hc-9 + fragments + 16 KiB clusters,
# kernel ≥6.6).
#
# `output` and `sourceDir` are raw shell fragments (e.g. "$out/foo.raw", tree).
{
  lib,
  erofs-utils,
  output,
  sourceDir,
  extraArgs ? [ ],
}:
let
  mkfs = "${erofs-utils}/bin/mkfs.erofs";
  fsck = "${erofs-utils}/bin/fsck.erofs";
  args = [
    "--all-root"
    "-T0"
    "-zlz4hc,level=9"
    "-Efragments"
    "-C"
    "16384"
  ]
  ++ extraArgs;
in
''
  SOURCE_DATE_EPOCH=0 ${mkfs} \
    ${lib.concatStringsSep " \\\n    " (map lib.escapeShellArg args)} \
    ${output} \
    ${sourceDir}
  ${fsck} ${output}
''
