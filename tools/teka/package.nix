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
}:
let
  common = {
    pname = "ouster-teka";
    version = "0.1.0";
    src = craneLib.cleanCargoSource ./.;
    strictDeps = true;
    CARGO_BUILD_TARGET = stdenv.hostPlatform.rust.rustcTarget;
    cargoExtraArgs = "--locked --workspace";
    nativeBuildInputs = [ pkg-config ];
    buildInputs = [ openssl ];
    doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
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
    fmt = craneLib.cargoFmt (builtins.removeAttrs common [ "cargoExtraArgs" ]);
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
    nativeBuildInputs = common.nativeBuildInputs ++ [ installShellFiles ];
    nativeCheckInputs = [
      bashInteractive
      fish
      zsh
      buildPackages.openssl
    ];
    postInstall = ''
      printf 'eval "$(%s completion bash)"\n' "$out/bin/teka" > teka.bash
      printf '%s completion fish | source\n' "$out/bin/teka" > teka.fish
      printf '#compdef teka\neval "$(%s completion zsh)"\nif [[ $funcstack[1] == _teka ]]; then\n  _teka "$@"\nfi\n' "$out/bin/teka" > teka.zsh
      installShellCompletion teka.{bash,fish,zsh}
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
