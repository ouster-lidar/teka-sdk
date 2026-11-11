{
  lib,
  stdenv,
  craneLib,
  python3,
}:
let
  common = {
    pname = "teka-runner";
    version = (builtins.fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;
    src = import ./source.nix { inherit lib craneLib; };
    cargoExtraArgs = "--locked -p teka --no-default-features --features app";
    cargoBuildExtraArgs = "--bin teka-runner";
    CARGO_BUILD_TARGET = stdenv.hostPlatform.rust.rustcTarget;
    strictDeps = true;
    doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
    RUSTFLAGS = "-Dwarnings";
  };
  cargoArtifacts = craneLib.buildDepsOnly common;
in
craneLib.buildPackage (
  common
  // {
    inherit cargoArtifacts;
    postCheck = lib.optionalString common.doCheck ''
      TEKA_RUNNER="$PWD/target/${stdenv.hostPlatform.rust.rustcTarget}/release/teka-runner" \
        ${python3}/bin/python3 crates/teka/tests/resource_process.py
    '';
    meta = {
      description = "Run an app with Teka registration, resource leases, and lifecycle management";
      mainProgram = "teka-runner";
      platforms = lib.platforms.unix;
    };
  }
)
