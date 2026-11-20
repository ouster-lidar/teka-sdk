{
  pkgs,
  sdk,
  crane,
  mkTeka,
  nixpkgsRev,
}:
let
  inherit (pkgs) lib stdenv;
  inherit (stdenv.hostPlatform) system;
  craneLib = crane.mkLib pkgs;
  portablePkgs = if stdenv.hostPlatform.isLinux then pkgs.pkgsStatic else pkgs;
  portable = (mkTeka portablePkgs).override { portable = true; };
  teka = (mkTeka pkgs).overrideAttrs (old: {
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
  asApp = package: {
    type = "app";
    program = lib.getExe package;
  };
  installerApp =
    published:
    asApp (
      pkgs.callPackage
        (if stdenv.hostPlatform.isDarwin then ./tests/installer-darwin.nix else ./tests/installer.nix)
        (
          {
            inherit (teka) dist;
            inherit published;
          }
          // lib.optionalAttrs stdenv.hostPlatform.isDarwin { inherit nixpkgsRev; }
        )
    );
  rustShell = craneLib.devShell {
    inputsFrom = [ teka ];
    packages =
      with pkgs;
      [
        bashInteractive
        fish
        zsh
        nixfmt
        deadnix
        nix-output-monitor
        openssl
        osslsigncode
      ]
      ++ lib.optionals stdenv.hostPlatform.isLinux [
        cryptsetup
        erofs-utils
        sbsigntool
      ];
  };
in
rec {
  packages = {
    inherit teka portable;
    teka-runner = pkgs.callPackage ./packages/teka-runner.nix { inherit craneLib; };
    teka-src = pkgs.callPackage ./packages/teka-src.nix { inherit craneLib; };
    fern = pkgs.callPackage ./packages/fern.nix { nodejs = pkgs.nodejs-slim_22; };
    publish-cache = pkgs.callPackage ./packages/publish-cache.nix { inherit sdk; };
    default = teka;
  };

  apps = {
    default = asApp teka;
  }
  // lib.optionalAttrs (system == "x86_64-linux" || system == "aarch64-darwin") {
    test-build-init = installerApp false;
    test-published-install = installerApp true;
  };

  checks = {
    inherit (packages) teka teka-runner;
    inherit (teka.cargoChecks) clippy fmt audit;
    teka-dist = pkgs.callPackage ./tests/dist.nix {
      inherit (teka) dist version;
      inherit system;
    };
  }
  // import ./tests/sdk.nix { inherit pkgs craneLib; };

  devShells = {
    rust = rustShell;
    default = pkgs.mkShell {
      inputsFrom = [ rustShell ];
      packages = [ teka ];
    };
  };
}
