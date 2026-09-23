{
  description = "Jetstream app image examples (erofs DDI)";

  inputs = {
    # Self-contained nixpkgs pin for this flake (toolchain + Python + GLFW).
    nixpkgs.url = "github:NixOS/nixpkgs/a50de1b7d8a586adc18d2395c19de7d6058e6030";
    crate2nix.url = "github:nix-community/crate2nix/5e1ecfd2d15b34ec90c2e51fdffbe8116595a767";
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
      appOutputs = (import ./nix/app-image.nix).flakeOutputs {
        nixpkgs = nixpkgsPkgs;
        inherit systems services appOverlays;
        ousterSdkSrc = ouster-sdk;
      };

      cargoNixDirs = [
        "examples/hello-pub"
        "examples/hello-sub"
        "examples/hello-slidev"
        "examples/hello-ouster-world"
        "examples/nvidia-devtools"
      ];
    in
    appOutputs
    // {
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
