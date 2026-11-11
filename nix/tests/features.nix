{
  lib,
  craneLib,
  openssl,
  pkg-config,
}:
lib.mapAttrs
  (
    name: args:
    craneLib.mkCargoDerivation {
      pname = "teka-${name}";
      version = (builtins.fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;
      src = import ../packages/source.nix { inherit lib craneLib; };
      cargoArtifacts = null;
      buildPhaseCargoCommand = "cargoWithProfile check --locked -p teka --all-targets ${args}";
      doInstallCargoArtifacts = false;
      nativeBuildInputs = [ pkg-config ];
      buildInputs = [ openssl ];
      strictDeps = true;
    }
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
