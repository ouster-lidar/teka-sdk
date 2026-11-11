{
  lib,
  runCommand,
  craneLib,
  tekaSrc,
}:

{
  crateName,
  src,
  cargoSrc ? src,
}:
let
  cargoRoot = "apps/examples/${crateName}";
  sdkSrc = if lib.isDerivation tekaSrc then tekaSrc else lib.cleanSource tekaSrc;
  # Cargo path dependencies must resolve identically in real and dependency builds.
  assemble =
    appSrc: sdkSrc:
    runCommand "${crateName}-source" { } ''
      mkdir -p "$out/${cargoRoot}" "$out/crates"
      cp -r ${appSrc}/. "$out/${cargoRoot}/"
      cp -r ${sdkSrc} "$out/crates/teka"
    '';
in
{
  inherit cargoRoot;
  cargoToml = "${cargoSrc}/Cargo.toml";
  cargoLock = "${cargoSrc}/Cargo.lock";
  src = assemble src sdkSrc;
  dummySrc = assemble (craneLib.mkDummySrc { src = cargoSrc; }) (
    sdkSrc.dummySrc or (craneLib.mkDummySrc { src = sdkSrc; })
  );
}
