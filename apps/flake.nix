{
  description = "Jetstream app image examples (erofs DDI)";

  inputs = {
    # Self-contained nixpkgs pin for this flake (toolchain + Python + GLFW).
    nixpkgs.url = "github:NixOS/nixpkgs/a50de1b7d8a586adc18d2395c19de7d6058e6030";
    crate2nix = {
      url = "github:nix-community/crate2nix/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.cachix.inputs.nixpkgs.follows = "nixpkgs";
    };
    # Public Ouster SDK (github.com/ouster-lidar/ouster-sdk). Built via
    # apps/nix/ouster-sdk.nix against nixpkgs GLFW.
    ouster-sdk = {
      url = "github:ouster-lidar/ouster-sdk/v1.0.1";
      flake = false;
    };
    jetpack = {
      url = "github:anduril/jetpack-nixos";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crate2nix,
      ouster-sdk,
      jetpack,
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
          overlays = appOverlays;
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

      exampleServices = import ./examples/flake-services.nix;
      services = exampleServices;

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
        inherit systems services appOverlays version;
        ousterSdkSrc = ouster-sdk;
      };

      cargoNixDirs = [
        "examples/hello-pub"
        "examples/hello-sub"
        "examples/hello-slidev"
        "examples/hello-ouster-world"
        "examples/hello-ouster-dynamo"
        "examples/nvidia-devtools"
      ];
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
      teka.apps = nixpkgs.lib.mapAttrs (_: _: { }) services;
      checks = nixpkgs.lib.genAttrs systems (
        system:
        let
          pkgs = nixpkgsPkgs.legacyPackages.${system};
          runtime = (infraPkgsFor system).app-runtime;
        in
        appOutputs.checks.${system}
        // nixpkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          app-runtime = runtime;
          hello-llm-runtime = pkgs.runCommand "hello-llm-runtime-check" { } ''
            APP_RUNTIME_PROCESS=${runtime}/bin/app-runtime-process \
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
          regen-cargo-nix = {
            type = "app";
            program = toString (
              pkgs.writeShellScript "regen-cargo-nix" ''
                set -euo pipefail
                crate2nix=${crate2nix.packages.${system}.default}/bin/crate2nix
                for d in ${nixpkgs.lib.concatStringsSep " " cargoNixDirs}; do
                  (cd "$d" && "$crate2nix" generate)
                done
              ''
            );
          };
          update-cargo-lock = {
            type = "app";
            program = toString (
              pkgs.writeShellScript "update-cargo-lock" ''
                set -euo pipefail
                cargo=${pkgs.cargo}/bin/cargo
                for d in ${nixpkgs.lib.concatStringsSep " " cargoNixDirs}; do
                  echo "updating $d/Cargo.lock"
                  (cd "$d" && "$cargo" update "$@")
                done
                echo
                echo "Next: nix run .#regen-cargo-nix"
              ''
            );
          };
        }
      );
    };
}
