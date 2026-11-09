{
  description = "Teka CLI";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.app-nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.ouster-sdk = {
    url = "github:ouster-lidar/ouster-sdk/v1.0.1";
    flake = false;
  };
  inputs.crane.url = "github:ipetkov/crane/master";
  inputs.teka = {
    url = "path:./tools/teka";
    inputs.nixpkgs.follows = "nixpkgs";
    inputs.crane.follows = "crane";
    inputs.rustsecAdvisoryDb.follows = "rustsecAdvisoryDb";
  };
  inputs.rustsecAdvisoryDb = {
    url = "github:RustSec/advisory-db";
    flake = false;
  };

  outputs =
    {
      self,
      nixpkgs,
      app-nixpkgs,
      ouster-sdk,
      crane,
      teka,
      ...
    }:
    let
      appLib = import ./apps/nix/app-lib.nix;
    in
    {
      lib = appLib // {
        mkAppOutputs = import ./apps/nix/app-outputs.nix {
          inherit
            appLib
            app-nixpkgs
            crane
            ouster-sdk
            ;
          tekaFor = system: self.packages.${system}.teka;
        };
      };
      packages = builtins.mapAttrs (_: packages: {
        inherit (packages) teka;
        default = packages.teka;
      }) teka.packages;
      inherit (teka) apps checks overlays;
      devShells = nixpkgs.lib.genAttrs (builtins.attrNames teka.packages) (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              teka.packages.${system}.teka
              nix-output-monitor
            ];
          };
        }
      );
    };
}
