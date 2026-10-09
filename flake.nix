{
  description = "Teka CLI";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.teka = {
    url = "path:./tools/teka";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      nixpkgs,
      teka,
      ...
    }:
    {
      packages = builtins.mapAttrs (_: packages: {
        inherit (packages) teka;
        default = packages.teka;
      }) teka.packages;
      inherit (teka) apps;
      overlays = teka.overlays // {
        default = teka.overlays.teka;
      };
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
