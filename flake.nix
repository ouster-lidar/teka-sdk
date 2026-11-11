{
  description = "Teka SDK and CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane/master";

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
      ouster-sdk,
      rustsecAdvisoryDb,
    }:
    let
      inherit (nixpkgs) lib;
      appLib = import ./nix/apps/app-lib.nix;
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
            import ./nix/default.nix {
              inherit pkgs crane;
              advisoryDb = rustsecAdvisoryDb;
              nixpkgsRev = nixpkgs.rev;
              sdkRef = if self ? rev then "github:ouster-lidar/teka-sdk/${self.rev}" else null;
            }
          );
      outputFor = name: lib.mapAttrs (_: outputs: outputs.${name}) perSystem;
    in
    {
      lib = appLib // {
        mkAppOutputs = import ./nix/apps/app-outputs.nix {
          inherit
            appLib
            nixpkgs
            crane
            ouster-sdk
            ;
          tekaFor = system: self.packages.${system}.teka;
        };
      };

      templates = (import ./apps/examples { inherit lib; }).templates // {
        default = self.templates.hello-world;
      };

      packages = outputFor "packages";
      apps = outputFor "apps";
      devShells = outputFor "devShells";

      checks = lib.recursiveUpdate (outputFor "checks") (import ./apps/tests { sdk = self; });

      overlays.default = final: _: {
        inherit (self.packages.${final.stdenv.hostPlatform.system}) teka teka-runner;
      };
    };
}
