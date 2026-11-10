{
  description = "Composite teka CLI";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.crane.url = "github:ipetkov/crane/master";
  inputs.rustsecAdvisoryDb = {
    url = "github:RustSec/advisory-db";
    flake = false;
  };
  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-unstable,
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
        pkgs: portable:
        pkgs.callPackage ./package.nix {
          inherit portable;
          sdkRef = if self ? rev then "github:ouster-lidar/teka-sdk/${self.rev}" else null;
          nixpkgsRev = nixpkgs.rev;
          nixpkgsUnstableRev = nixpkgs-unstable.rev;
          craneLib = crane.mkLib pkgs;
          advisoryDb = rustsecAdvisoryDb;
          openssl =
            if portable && pkgs.stdenv.hostPlatform.isDarwin then
              pkgs.openssl.override { static = true; }
            else
              pkgs.openssl;
          verifierEnv =
            if pkgs.stdenv.hostPlatform.isLinux then
              { TEKA_SBVERIFY = pkgs.lib.getExe' pkgs.sbsigntool "sbverify"; }
            else
              { TEKA_OSSLSIGNCODE = pkgs.lib.getExe pkgs.osslsigncode; };
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          teka = (mkTeka pkgs false).overrideAttrs (old: {
            passthru = old.passthru // {
              dist =
                pkgs.runCommand "teka-dist-${portable.version}"
                  {
                    allowedReferences = [ ];
                    meta.mainProgram = "teka";
                  }
                  ''
                    install -Dm755 ${portable}/bin/teka "$out/teka-${system}"
                    cd "$out"
                    sha256sum teka-${system} > SHA256SUMS
                    mkdir bin
                    ln -s ../teka-${system} bin/teka
                    ln -s teka bin/teka-${system}
                  '';
            };
          });
          portable = mkTeka (if pkgs.stdenv.hostPlatform.isLinux then pkgs.pkgsStatic else pkgs) true;
        in
        {
          inherit teka portable;
          default = teka;
        }
      );
      apps = forAllSystems (
        system:
        {
          teka = {
            type = "app";
            program = "${self.packages.${system}.teka}/bin/teka";
          };
          default = self.apps.${system}.teka;
        }
        // nixpkgs.lib.optionalAttrs (system == "x86_64-linux" || system == "aarch64-darwin") {
          test-build-init = {
            type = "app";
            program = nixpkgs.lib.getExe (
              let
                pkgs = import nixpkgs {
                  inherit system;
                  config.allowUnfreePredicate = pkg: nixpkgs.lib.getName pkg == "tart";
                };
              in
              pkgs.callPackage
                (if system == "aarch64-darwin" then ./tests/installer-darwin.nix else ./tests/installer.nix)
                (
                  {
                    dist = self.packages.${system}.teka.dist;
                  }
                  // nixpkgs.lib.optionalAttrs (system == "aarch64-darwin") {
                    nixpkgsRev = nixpkgs.rev;
                  }
                )
            );
          };
        }
      );
      overlays.default = final: _: {
        teka = self.packages.${final.stdenv.hostPlatform.system}.teka;
      };
      checks = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          teka = self.packages.${system}.teka;
        in
        {
          inherit teka;
          inherit (teka.cargoChecks) clippy fmt audit;
          teka-dist = pkgs.callPackage ./tests/dist.nix {
            dist = teka.dist;
            inherit system;
            version = teka.version;
          };
        }
      );
      devShells = forAllSystems (system: {
        default = nixpkgs.legacyPackages.${system}.mkShell {
          buildInputs = [ nixpkgs.legacyPackages.${system}.openssl ];
          packages =
            with nixpkgs.legacyPackages.${system};
            [
              bashInteractive
              cargo
              fish
              clippy
              rustc
              rustfmt
              zsh
              nixfmt
              openssl
              osslsigncode
              pkg-config
            ]
            ++ nixpkgs.lib.optionals (nixpkgs.lib.hasSuffix "-linux" system) (
              with nixpkgs.legacyPackages.${system};
              [
                cryptsetup
                erofs-utils
                sbsigntool
              ]
            );
        };
      });
    };
}
