# Standalone nix build of the shared app runtime crate (for flake consumers).
{
  rustPlatform,
  lib,
  python3,
  stdenv,
}:

rustPlatform.buildRustPackage rec {
  pname = "app-runtime";
  version = "0.1.0";
  src = lib.cleanSource ../lib/app-runtime;

  cargoLock = {
    lockFile = ../lib/app-runtime/Cargo.lock;
    outputHashes = { };
  };

  RUSTFLAGS = "-Dwarnings";

  postCheck = lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
    APP_RUNTIME_PROCESS="$PWD/target/${stdenv.hostPlatform.rust.rustcTarget}/release/app-runtime-process" \
      ${python3}/bin/python3 tests/resource_process.py
  '';

  postInstall = ''
    mkdir -p $out/lib
    rlib=$(
      find "''${CARGO_TARGET_DIR:-target}" -path '*/release/deps/libapp_runtime-*.rlib' -print -quit
    )
    if [ -z "$rlib" ]; then
      echo "postInstall: libapp_runtime rlib not found" >&2
      exit 1
    fi
    cp "$rlib" $out/lib/libapp_runtime.rlib
  '';
}
