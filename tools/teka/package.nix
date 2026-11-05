{
  lib,
  stdenv,
  craneLib,
  advisoryDb ? null,
  buildPackages,
  installShellFiles,
  bashInteractive,
  fish,
  zsh,
  openssl,
  pkg-config,
  removeReferencesTo,
  portable ? false,
  nixpkgsRev ? (builtins.fromJSON (builtins.readFile ./flake.lock)).nodes.nixpkgs.locked.rev,
  verifierEnv,
}:
let
  common = {
    pname = "teka-cli";
    version = (builtins.fromTOML (builtins.readFile ./cli/Cargo.toml)).package.version;
    src = lib.cleanSourceWith {
      src = ./.;
      filter = path: type: craneLib.filterCargoSources path type || lib.hasSuffix "/flake.lock" path;
    };
    strictDeps = true;
    CARGO_BUILD_TARGET = stdenv.hostPlatform.rust.rustcTarget;
    cargoExtraArgs = "--locked --workspace";
    doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
    nativeBuildInputs = [ pkg-config ];
    buildInputs = [ openssl ];
    TEKA_VM_NIXPKGS_REV = nixpkgsRev;
  }
  // lib.optionalAttrs (!portable) (
    verifierEnv
    // {
      TEKA_NIX_PACKAGE = "1";
    }
  )
  // lib.optionalAttrs stdenv.hostPlatform.isDarwin {
    RUSTFLAGS = "-C link-arg=-Wl,-dead_strip_dylibs";
  };
  cargoArtifacts = craneLib.buildDepsOnly common;
  cargoChecks = {
    clippy = craneLib.cargoClippy (
      common
      // {
        inherit cargoArtifacts;
        cargoClippyExtraArgs = "--all-targets -- -D warnings";
      }
    );
    fmt = craneLib.cargoFmt {
      inherit (common) pname version src;
    };
  }
  // lib.optionalAttrs (advisoryDb != null) {
    audit = craneLib.cargoAudit {
      src = common.src;
      advisory-db = advisoryDb;
      pname = common.pname;
      version = common.version;
    };
  };
in
craneLib.buildPackage (
  common
  // {
    inherit cargoArtifacts;
    nativeBuildInputs =
      common.nativeBuildInputs
      ++ [
        installShellFiles
      ]
      ++ lib.optional portable removeReferencesTo;
    nativeCheckInputs = [
      bashInteractive
      fish
      zsh
    ]
    ++ (
      with buildPackages;
      [
        openssl
        osslsigncode
      ]
      ++ lib.optionals stdenv.hostPlatform.isLinux [
        cryptsetup
        sbsigntool
      ]
    );
    postInstall = ''
      printf 'eval "$(%s completion bash)"\n' "$out/bin/teka" > teka.bash
      printf '%s completion fish | source\n' "$out/bin/teka" > teka.fish
      printf '#compdef teka\neval "$(%s completion zsh)"\nif [[ $funcstack[1] == _teka ]]; then\n  _teka "$@"\nfi\n' "$out/bin/teka" > teka.zsh
      installShellCompletion teka.{bash,fish,zsh}
    '';
    postFixup = lib.optionalString portable ''
      # Package signing uses explicit certificates and OpenSSL's built-in provider.
      remove-references-to -t ${lib.getOutput "etc" openssl} "$out/bin/teka"
    '';
    passthru = {
      inherit cargoChecks;
      clippy = cargoChecks.clippy;
    };
    meta = {
      description = "Composite CLI for Teka devices";
      mainProgram = "teka";
      platforms = lib.platforms.unix;
    };
  }
)
