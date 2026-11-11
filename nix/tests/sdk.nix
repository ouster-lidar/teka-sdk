{
  lib,
  stdenv,
  craneLib,
  openssl,
  pkg-config,
  osslsigncode,
  cryptsetup,
  sbsigntool,
}:
let
  common = {
    pname = "teka-sdk";
    version = (builtins.fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;
    src = import ../packages/source.nix { inherit lib craneLib; };
    cargoExtraArgs = "--locked -p teka --all-features";
    CARGO_BUILD_TARGET = stdenv.hostPlatform.rust.rustcTarget;
    nativeBuildInputs = [ pkg-config ];
    buildInputs = [ openssl ];
    strictDeps = true;
  };
  cargoArtifacts = craneLib.buildDepsOnly common;
in
{
  sdk-tests = craneLib.cargoTest (
    common
    // {
      inherit cargoArtifacts;
      doInstallCargoArtifacts = false;
      nativeCheckInputs = [
        openssl
        osslsigncode
      ]
      ++ lib.optionals stdenv.hostPlatform.isLinux [
        cryptsetup
        sbsigntool
      ];
    }
  );
  sdk-clippy = craneLib.cargoClippy (
    common
    // {
      inherit cargoArtifacts;
      cargoClippyExtraArgs = "--all-targets -- -D warnings";
      doInstallCargoArtifacts = false;
    }
  );
  sdk-macro = craneLib.mkCargoDerivation (
    common
    // {
      inherit cargoArtifacts;
      pname = "teka-sdk-macro";
      # The independent consumer resolves only dependencies vendored from the workspace lockfile.
      buildPhaseCargoCommand = "cargoWithProfile check --offline --manifest-path crates/teka/tests/fixtures/macro/Cargo.toml --target-dir target";
      doInstallCargoArtifacts = false;
    }
  );
}
