{
  pkgs,
  crane,
  advisoryDb,
  nixpkgsRev,
  sdkRef,
}:
let
  inherit (pkgs) lib stdenv;
  inherit (stdenv.hostPlatform) system;
  craneLib = crane.mkLib pkgs;
  mkTeka =
    pkgs: portable:
    pkgs.callPackage ./packages/teka-cli.nix {
      inherit
        portable
        nixpkgsRev
        sdkRef
        advisoryDb
        ;
      craneLib = crane.mkLib pkgs;
    };
  portable = mkTeka (if stdenv.hostPlatform.isLinux then pkgs.pkgsStatic else pkgs) true;
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
    default = teka;
  };

  apps = {
    teka = {
      type = "app";
      program = lib.getExe teka;
    };
    default = apps.teka;
  }
  // lib.optionalAttrs (system == "x86_64-linux" || system == "aarch64-darwin") {
    test-build-init = {
      type = "app";
      program = lib.getExe (
        pkgs.callPackage
          (if stdenv.hostPlatform.isDarwin then ./tests/installer-darwin.nix else ./tests/installer.nix)
          ({ inherit (teka) dist; } // lib.optionalAttrs stdenv.hostPlatform.isDarwin { inherit nixpkgsRev; })
      );
    };
  };

  checks = {
    inherit (packages) teka teka-runner;
    inherit (teka.cargoChecks) clippy fmt audit;
    teka-dist = pkgs.callPackage ./tests/dist.nix {
      inherit (teka) dist version;
      inherit system;
    };
  }
  // import ./tests/sdk.nix {
    inherit craneLib;
    inherit (pkgs)
      lib
      stdenv
      openssl
      pkg-config
      osslsigncode
      cryptsetup
      sbsigntool
      ;
  }
  // import ./tests/features.nix {
    inherit craneLib;
    inherit (pkgs) lib openssl pkg-config;
  };

  devShells = {
    rust = rustShell;
    default = pkgs.mkShell {
      inputsFrom = [ rustShell ];
      packages = [ teka ];
    };
  };
}
