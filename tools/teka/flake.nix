{
  description = "Composite teka CLI";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.crane.url = "github:ipetkov/crane/master";
  outputs =
    {
      self,
      nixpkgs,
      crane,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      mkTeka = pkgs: pkgs.callPackage ./package.nix { craneLib = crane.mkLib pkgs; };
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
      checks = forAllSystems (system: {
        inherit (self.packages.${system}) teka;
        clippy = self.packages.${system}.teka.clippy;
      });
      devShells = forAllSystems (system: {
        default = nixpkgs.legacyPackages.${system}.mkShell {
          packages = with nixpkgs.legacyPackages.${system}; [
            cargo
            clippy
            rustc
            rustfmt
            nixfmt
          ];
        };
      });
    };
}
