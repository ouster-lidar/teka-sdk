{
  lib,
  stdenv,
  craneLib,
}:
let
  common = {
    pname = "ouster-teka";
    version = "0.1.0";
    src = craneLib.cleanCargoSource ./.;
    strictDeps = true;
    CARGO_BUILD_TARGET = stdenv.hostPlatform.rust.rustcTarget;
    cargoExtraArgs = "--locked -p ouster-teka";
    doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  };
  cargoArtifacts = craneLib.buildDepsOnly common;
in
craneLib.buildPackage (
  common
  // {
    inherit cargoArtifacts;
    passthru = {
      clippy = craneLib.cargoClippy (
        common
        // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- -D warnings";
        }
      );
    };
    meta = {
      description = "Composite CLI for Teka devices";
      mainProgram = "teka";
      platforms = lib.platforms.unix;
    };
  }
)
