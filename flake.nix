{
  description = "Teka SDK and CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane/master";

    jetpack = {
      url = "github:anduril/jetpack-nixos";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    ouster-sdk = {
      url = "github:ouster-lidar/ouster-sdk/v1.0.1";
      flake = false;
    };

    rustsecAdvisoryDb = {
      url = "github:RustSec/advisory-db";
      flake = false;
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      jetpack,
      ouster-sdk,
      rustsecAdvisoryDb,
    }:
    let
      inherit (nixpkgs) lib;
      perSystem =
        lib.genAttrs
          [
            "x86_64-linux"
            "aarch64-linux"
            "aarch64-darwin"
          ]
          (
            system:
            let
              pkgs = import nixpkgs {
                inherit system;
                config.allowUnfreePredicate = pkg: lib.getName pkg == "tart";
              };
            in
            import ./nix {
              inherit pkgs crane;
              advisoryDb = rustsecAdvisoryDb;
              nixpkgsRev = nixpkgs.rev;
              sdkRef = if self ? rev then "github:ouster-lidar/teka-sdk/${self.rev}" else null;
            }
          );
      examples = import ./apps {
        sdk = self;
        inherit jetpack;
      };
      outputFor = name: lib.mapAttrs (_: outputs: outputs.${name}) perSystem;
    in
    {
      lib = {
        mkVersion = import ./nix/apps/version.nix;
        paths = {
          erofsMkfs = ./nix/apps/erofs-mkfs.nix;
          coreVersionValidate = ./nix/apps/core-version-validate.nix;
        };
        mkAppOutputs = import ./nix/apps {
          inherit
            nixpkgs
            crane
            ouster-sdk
            ;
          tekaFor = system: self.packages.${system}.teka;
        };
      };

      inherit (examples) teka;

      templates = examples.templates // {
        default = self.templates.hello-world;
      };

      packages = outputFor "packages";
      apps = lib.recursiveUpdate (outputFor "apps") examples.apps;
      devShells = outputFor "devShells";

      checks = lib.recursiveUpdate (outputFor "checks") examples.checks;

      overlays.default = final: _: {
        inherit (self.packages.${final.stdenv.hostPlatform.system}) teka teka-runner;
      };
    };
}
