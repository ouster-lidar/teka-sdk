{
  description = "Teka app image examples (erofs DDI)";

  inputs = {
    # Self-contained nixpkgs pin for this flake (toolchain + Python + GLFW).
    nixpkgs.url = "github:NixOS/nixpkgs/a50de1b7d8a586adc18d2395c19de7d6058e6030";
    crane.url = "github:ipetkov/crane/master";
    rustsecAdvisoryDb = {
      url = "github:RustSec/advisory-db";
      flake = false;
    };
    # Public Ouster SDK (github.com/ouster-lidar/ouster-sdk). Built via
    # apps/nix/ouster/ouster-sdk.nix against nixpkgs GLFW.
    ouster-sdk = {
      url = "github:ouster-lidar/ouster-sdk/v1.0.1";
      flake = false;
    };
    jetpack = {
      url = "github:anduril/jetpack-nixos";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    teka = {
      url = "path:../tools/teka";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.crane.follows = "crane";
      inputs.rustsecAdvisoryDb.follows = "rustsecAdvisoryDb";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rustsecAdvisoryDb,
      ouster-sdk,
      jetpack,
      teka,
      ...
    }:
    let
      jp7Overlay =
        final: prev:
        {
          nvidia-jetpack = final.nvidia-jetpack7;
          cudaPackages = prev.cudaPackages_13_2;
        };

      appOverlays = [
        jetpack.overlays.default
        jp7Overlay
      ];

      nixpkgsFor =
        system:
        import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      nixpkgsPkgs = nixpkgs // {
        lib = nixpkgs.lib;
        legacyPackages = nixpkgs.lib.genAttrs systems nixpkgsFor;
      };

      exampleApps = import ./examples { lib = nixpkgs.lib; };
      inherit (exampleApps) services imageChecks cargoDirs;

      # These examples can cross-build without the heavier package workarounds.
      simpleCrossServices = [
        "hello-world"
        "hello-http"
        "hello-llm"
        "nvidia-devtools"
      ];

      appLib = import ./nix/app-lib.nix;

      version = appLib.mkVersion {
        inherit self;
        inherit (nixpkgs) lib;
      };

      infraPkgsFor =
        system:
        let
          pkgs = nixpkgsPkgs.legacyPackages.${system};
        in
        {
          app-runtime = pkgs.callPackage ./nix/app-runtime.nix { };
          app-api-js = pkgs.callPackage ./nix/app-api-js.nix { };
          app-runtime-src = pkgs.callPackage ./nix/app-runtime-src.nix { };
        };

      appOutputs = appLib.flakeOutputs {
        nixpkgs = nixpkgsPkgs;
        inherit
          crane
          systems
          services
          appOverlays
          simpleCrossServices
          imageChecks
          version
          ;
        advisoryDb = rustsecAdvisoryDb;
        ousterSdkSrc = ouster-sdk;
        tekaFor = system: teka.packages.${system}.teka;
      };
    in
    let
      infraPackages = nixpkgs.lib.mapAttrs (
        system: attrs:
        attrs // infraPkgsFor system
      ) appOutputs.packages;
    in
    appOutputs
    // {
      packages = infraPackages;
      teka.apps = exampleApps.apps;
      devShells = nixpkgs.lib.genAttrs systems (system: {
        default = nixpkgs.legacyPackages.${system}.mkShell {
          packages = [ teka.packages.${system}.teka ];
        };
      });
      checks = nixpkgs.lib.genAttrs systems (
        system:
        let
          pkgs = nixpkgsPkgs.legacyPackages.${system};
          runtime = (infraPkgsFor system).app-runtime;
        in
        appOutputs.checks.${system}
        // nixpkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          app-runtime = runtime;
          signed-images = pkgs.callPackage ./nix/tests/signed-images.nix { inherit version; };
          flake-output-overlays = pkgs.callPackage ./nix/tests/flake-output-overlays.nix {
            tekaFor = system: teka.packages.${system}.teka;
            inherit
              appLib
              crane
              nixpkgs
              version
              ;
          };
          hello-llm-runtime = pkgs.runCommand "hello-llm-runtime-check" { } ''
            TEKA_RUNNER=${runtime}/bin/teka-runner \
              ${pkgs.python3}/bin/python3 ${./.}/examples/hello-llm/tests/runtime.py
            touch "$out"
          '';
          hello-ouster-world-startup = pkgs.runCommand "hello-ouster-world-startup-check" { } ''
            ${pkgs.python3}/bin/python3 ${./.}/examples/hello-ouster-world/tests/startup.py
            touch "$out"
          '';
        }
      );
      lib = appLib // {
        inherit (version) appArtifactRawName imageVersion buildId compose;
      };
      apps = nixpkgs.lib.genAttrs systems (
        system:
        let
          pkgs = nixpkgsPkgs.legacyPackages.${system};
        in
        {
          update-cargo-lock = {
            type = "app";
            program = toString (
              pkgs.writeShellScript "update-cargo-lock" ''
                set -euo pipefail
                cargo=${pkgs.cargo}/bin/cargo
                for d in ${nixpkgs.lib.concatStringsSep " " cargoDirs}; do
                  echo "updating $d/Cargo.lock"
                  (cd "$d" && "$cargo" update "$@")
                done
              ''
            );
          };
        }
      );
    };
}
