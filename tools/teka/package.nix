{
  lib,
  stdenv,
  craneLib,
  installShellFiles,
  bash,
  fish,
  zsh,
}:
let
  common = {
    pname = "ouster-teka";
    version = "0.1.0";
    src = craneLib.cleanCargoSource ./.;
    strictDeps = true;
    CARGO_BUILD_TARGET = stdenv.hostPlatform.rust.rustcTarget;
    cargoExtraArgs = "--locked -p ouster-teka";
    doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  };
  cargoArtifacts = craneLib.buildDepsOnly common;
in
craneLib.buildPackage (
  common
  // {
    inherit cargoArtifacts;
    nativeBuildInputs = [ installShellFiles ];
    nativeCheckInputs = [
      bash
      fish
      zsh
    ];
    postInstall = ''
      printf 'eval "$(%s completion bash)"\n' "$out/bin/teka" > teka.bash
      printf '%s completion fish | source\n' "$out/bin/teka" > teka.fish
      printf '#compdef teka\neval "$(%s completion zsh)"\nif [[ $funcstack[1] == _teka ]]; then\n  _teka "$@"\nfi\n' "$out/bin/teka" > teka.zsh
      installShellCompletion teka.{bash,fish,zsh}
    '';
    passthru = {
      clippy = craneLib.cargoClippy (
        common
        // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- -D warnings";
        }
      );
    };
    meta = {
      description = "Composite CLI for Teka devices";
      mainProgram = "teka";
      platforms = lib.platforms.unix;
    };
  }
)
