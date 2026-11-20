{ pkgs, craneLib }:
let
  inherit (pkgs) lib stdenv;
  common = {
    version = (builtins.fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;
    src = import ../packages/source.nix { inherit lib craneLib; };
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.openssl ];
    strictDeps = true;
  };
  sdk = common // {
    pname = "teka-sdk";
    cargoExtraArgs = "--locked -p teka --all-features";
    CARGO_BUILD_TARGET = stdenv.hostPlatform.rust.rustcTarget;
  };
  sdkCheck = sdk // {
    cargoArtifacts = craneLib.buildDepsOnly sdk;
    doInstallCargoArtifacts = false;
  };
in
{
  sdk-tests = craneLib.cargoTest (
    sdkCheck
    // {
      nativeCheckInputs = [
        pkgs.openssl
        pkgs.osslsigncode
      ]
      ++ lib.optionals stdenv.hostPlatform.isLinux [
        pkgs.cryptsetup
        pkgs.sbsigntool
      ];
    }
  );
  sdk-clippy = craneLib.cargoClippy (
    sdkCheck
    // {
      cargoClippyExtraArgs = "--all-targets -- -D warnings";
    }
  );
  sdk-macro = craneLib.mkCargoDerivation (
    sdkCheck
    // {
      pname = "teka-sdk-macro";
      # The independent consumer resolves only dependencies vendored from the workspace lockfile.
      buildPhaseCargoCommand = "cargoWithProfile check --offline --manifest-path crates/teka/tests/fixtures/macro/Cargo.toml --target-dir target";
    }
  );
}
//
  lib.mapAttrs
    (
      name: args:
      craneLib.mkCargoDerivation (
        common
        // {
          pname = "teka-${name}";
          cargoArtifacts = null;
          buildPhaseCargoCommand = "cargoWithProfile check --locked -p teka --all-targets ${args}";
          doInstallCargoArtifacts = false;
        }
      )
    )
    {
      sdk-minimal = "--no-default-features";
      sdk-default = "";
      sdk-app = "--no-default-features --features app";
      sdk-client = "--no-default-features --features client";
      sdk-tpk = "--no-default-features --features tpk";
      sdk-app-client = "--no-default-features --features app,client";
      sdk-app-tpk = "--no-default-features --features app,tpk";
      sdk-client-tpk = "--no-default-features --features client,tpk";
      sdk-all = "--all-features";
    }
