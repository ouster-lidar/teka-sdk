{
  lib,
  stdenv,
  buildPackages,
  craneLib,
  advisoryDb ? null,
}:

let
  inherit (buildPackages) runCommand;
  tekaSrc = buildPackages.callPackage ../packages/teka-src.nix { inherit craneLib; };

  buildRustApp =
    {
      srcDir ? null,
      src ? null,
      nativeBuildInputs ? [ ],
      buildInputs ? [ ],
      meta ? { },
    }:
    assert lib.assertMsg (
      (srcDir == null) != (src == null)
    ) "buildRustApp requires exactly one of srcDir or src";
    let
      appSrc =
        if srcDir != null then
          lib.sources.sourceByRegex srcDir [
            "Cargo.lock"
            "Cargo.toml"
            "build[.]rs"
            "[.]cargo(/config([.]toml)?)?"
            "(src|tests|examples|benches)(/.*)?"
          ]
        else
          src;
      # Read Cargo metadata without materializing the filtered source.
      sourceRoot = if appSrc._isLibCleanSourceWith or false then appSrc.origSrc else appSrc;
      cargoToml = sourceRoot + "/Cargo.toml";
      cargoLock = sourceRoot + "/Cargo.lock";
      manifest = builtins.fromTOML (builtins.readFile cargoToml);
      inherit (manifest.package) name version;
      cargoRoot = "apps/examples/${name}";
      # Keep Cargo path dependencies at the same relative paths in both builds.
      assemble =
        app: sdk:
        runCommand "${name}-source" { } ''
          mkdir -p "$out/${cargoRoot}" "$out/crates"
          cp -r -- ${lib.escapeShellArg "${app}/."} "$out/${cargoRoot}/"
          cp -r -- ${lib.escapeShellArg "${sdk}"} "$out/crates/teka"
        '';
      dummySrc = assemble (craneLib.mkDummySrc { src = appSrc; }) tekaSrc.dummySrc;
      common = {
        pname = name;
        src = assemble appSrc tekaSrc;
        inherit
          version
          cargoToml
          nativeBuildInputs
          buildInputs
          ;
        cargoVendorDir = craneLib.vendorCargoDeps { inherit cargoLock; };
        postUnpack = "sourceRoot+=/${lib.escapeShellArg cargoRoot}";
        strictDeps = true;
        doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
        # Cargo caps dependency lints, leaving warnings fatal for our local crates.
        RUSTFLAGS = "-Dwarnings";
      };
      cargoArtifacts = craneLib.buildDepsOnly (removeAttrs common [ "src" ] // { inherit dummySrc; });
      tests = {
        clippy = craneLib.cargoClippy (
          common
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          }
        );
        fmt = craneLib.cargoFmt common;
      }
      // lib.optionalAttrs (advisoryDb != null) {
        audit = craneLib.cargoAudit {
          src = appSrc;
          advisory-db = advisoryDb;
          inherit (common) pname version;
        };
      };
    in
    craneLib.buildPackage (
      common
      // {
        inherit cargoArtifacts;
        passthru = { inherit tests; };
        meta = {
          mainProgram = manifest.package.default-run or name;
        }
        // meta;
      }
    );
in
buildRustApp
