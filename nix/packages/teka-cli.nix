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
  osslsigncode,
  pkg-config,
  sbsigntool,
  removeReferencesTo,
  portable ? false,
  sdkRef ? null,
  nixpkgsRev ? (builtins.fromJSON (builtins.readFile ../../flake.lock)).nodes.nixpkgs.locked.rev,
  verifierEnv ?
    if stdenv.hostPlatform.isLinux then
      { TEKA_SBVERIFY = lib.getExe' sbsigntool "sbverify"; }
    else
      { TEKA_OSSLSIGNCODE = lib.getExe osslsigncode; },
}:
let
  opensslPackage =
    if portable && stdenv.hostPlatform.isDarwin then openssl.override { static = true; } else openssl;
  common = {
    pname = "teka-cli";
    version = (builtins.fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;
    src = import ./source.nix {
      inherit lib craneLib;
      cli = true;
    };
    strictDeps = true;
    CARGO_BUILD_TARGET = stdenv.hostPlatform.rust.rustcTarget;
    cargoExtraArgs = "--locked -p teka-cli";
    doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
    nativeBuildInputs = [ pkg-config ];
    buildInputs = [ opensslPackage ];
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
    TEKA_SDK_REF = sdkRef;
    nativeBuildInputs =
      common.nativeBuildInputs
      ++ [
        installShellFiles
      ]
      ++ lib.optional portable removeReferencesTo;
    nativeCheckInputs = [
      buildPackages.gitMinimal
      bashInteractive
      fish
      zsh
      opensslPackage
      buildPackages.osslsigncode
    ]
    ++ lib.optionals stdenv.hostPlatform.isLinux [
      buildPackages.cryptsetup
      buildPackages.sbsigntool
    ];
    postInstall = ''
      ${lib.optionalString (stdenv.hostPlatform.isDarwin && stdenv.hostPlatform.isAarch64) ''
        ln -s teka "$out/bin/teka-build-vm"
      ''}
      printf 'eval "$(%s completion bash)"\n' "$out/bin/teka" > teka.bash
      printf '%s completion fish | source\n' "$out/bin/teka" > teka.fish
      printf '#compdef teka\neval "$(%s completion zsh)"\nif [[ $funcstack[1] == _teka ]]; then\n  _teka "$@"\nfi\n' "$out/bin/teka" > teka.zsh
      installShellCompletion teka.{bash,fish,zsh}
    '';
    postFixup = lib.optionalString portable ''
      # Package signing uses explicit certificates and OpenSSL's built-in provider.
      remove-references-to -t ${lib.getOutput "etc" opensslPackage} "$out/bin/teka"
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
