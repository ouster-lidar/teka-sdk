# Canonical mkfs.erofs flags for app image DDIs.
#
# Same tuning as Teka appliance rootfs (lz4hc-9 + fragments + 16 KiB clusters,
# kernel ≥6.6). Shared with downstream appliance image builders.
#
# `output` and `sourceDir` are raw shell fragments (e.g. "$out/foo.raw", tree).
{ lib, erofs-utils }:
let
  args = [
    "--all-root"
    "-T0"
    "-Efragments"
    "-C"
    "16384"
  ];
in
{
  inherit args;

  mkfs =
    {
      output,
      sourceDir,
      extraArgs ? [ ],
    }:
    let
      flags = args ++ [ "-zlz4hc,level=9" ] ++ extraArgs;
    in
    ''
      SOURCE_DATE_EPOCH=0 ${erofs-utils}/bin/mkfs.erofs \
        ${lib.escapeShellArgs flags} \
        ${output} \
        ${sourceDir}
      ${erofs-utils}/bin/fsck.erofs ${output}
    '';
}
