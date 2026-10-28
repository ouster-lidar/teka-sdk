{
  description = "Composite teka CLI";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.crane.url = "github:ipetkov/crane/master";
  inputs.rustsecAdvisoryDb = {
    url = "github:RustSec/advisory-db";
    flake = false;
  };
  outputs =
    {
      self,
      nixpkgs,
      crane,
      rustsecAdvisoryDb,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      mkTeka =
        pkgs:
        pkgs.callPackage ./package.nix {
          craneLib = crane.mkLib pkgs;
          advisoryDb = rustsecAdvisoryDb;
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          teka = mkTeka pkgs;
        in
        {
          inherit teka;
          default = teka;
          portable = if pkgs.stdenv.hostPlatform.isLinux then mkTeka pkgs.pkgsStatic else teka;
        }
      );
      apps = forAllSystems (system: {
        teka = {
          type = "app";
          program = "${self.packages.${system}.teka}/bin/teka";
        };
        default = self.apps.${system}.teka;
      });
      overlays.default = final: _: {
        teka = self.packages.${final.stdenv.hostPlatform.system}.teka;
      };
      checks = forAllSystems (
        system:
        let
          teka = self.packages.${system}.teka;
        in
        {
          inherit teka;
          inherit (teka.cargoChecks) clippy fmt audit;
        }
      );
      devShells = forAllSystems (system: {
        default = nixpkgs.legacyPackages.${system}.mkShell {
          buildInputs = [ nixpkgs.legacyPackages.${system}.openssl ];
          packages = with nixpkgs.legacyPackages.${system}; [
            bashInteractive
            cargo
            fish
            clippy
            openssl
            pkg-config
            zsh
            rustc
            rustfmt
            nixfmt
          ];
        };
      });
    };
}
