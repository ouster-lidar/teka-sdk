{
  description = "Teka SDK and CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane/master";

    jetpack = {
      url = "github:anduril/jetpack-nixos";
      inputs.nixpkgs.follows = "nixpkgs";
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
      rustsecAdvisoryDb,
    }:
    let
      inherit (nixpkgs) lib;
      nixpkgsRev = nixpkgs.rev;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      mkTeka =
        pkgs:
        pkgs.callPackage ./nix/packages/teka-cli.nix {
          inherit nixpkgsRev;
          craneLib = crane.mkLib pkgs;
          advisoryDb = rustsecAdvisoryDb;
          sdkRef = if self ? rev then "github:ouster-lidar/teka-sdk/${self.rev}" else null;
        };
      perSystem = lib.genAttrs systems (
        system:
        import ./nix {
          sdk = self;
          inherit crane mkTeka nixpkgsRev;
          pkgs = import nixpkgs {
            inherit system;
            config.allowUnfreePredicate = pkg: lib.getName pkg == "tart";
          };
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
        certs.app.dev = builtins.readFile ./nix/apps/dev-keys/apps.pem;
        inherit (import ./nix/apps/erofs-mkfs.nix { inherit lib; }) mkErofsImageScript;
        mkAppOutputs = import ./nix/apps {
          inherit nixpkgs crane rustsecAdvisoryDb;
          tekaFor = system: self.packages.${system}.teka;
        };
      };

      inherit (examples) teka;

      templates = examples.templates // {
        default = examples.templates.hello-world;
      };

      packages = lib.mapAttrs (
        system: outputs: (examples.packages.${system} or { }) // outputs.packages
      ) perSystem;
      apps = lib.recursiveUpdate (outputFor "apps") examples.apps;
      devShells = outputFor "devShells";
      checks = lib.recursiveUpdate (outputFor "checks") examples.checks;

      overlays.default =
        final: _:
        let
          inherit (final.stdenv) buildPlatform hostPlatform;
        in
        {
          teka =
            if buildPlatform == hostPlatform then self.packages.${hostPlatform.system}.teka else mkTeka final;
        };
    };
}
