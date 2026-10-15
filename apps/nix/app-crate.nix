{
  ps,
  pkgs,
  lib,
  runCommand,
  craneLib,
  appApiJs ? ../web/app-api.js,
  appRuntimeSrc ? ../lib/app-runtime,
}:

let
  appApiJsSrc =
    if lib.isDerivation appApiJs then
      "${appApiJs}/app-api.js"
    else
      builtins.path {
        path = appApiJs;
        name = "app-api.js";
      };

  buildAppCrate =
    {
      crateName,
      filteredSrc,
      nativeBuildInputs ? [ ],
      buildInputs ? [ ],
      meta ? { },
    }:
    let
      runtimeSrc =
        if lib.isDerivation appRuntimeSrc then appRuntimeSrc else lib.cleanSource appRuntimeSrc;
      # Preserve Cargo's relative path dependency, including downstream overrides.
      mkSource =
        appSrc: runtimeSrc:
        pkgs.buildPackages.runCommand "${crateName}-source" { } ''
          mkdir -p "$out/examples/${crateName}" "$out/lib"
          cp -r ${appSrc}/. "$out/examples/${crateName}/"
          cp -r ${runtimeSrc} "$out/lib/app-runtime"
        '';
      src = mkSource filteredSrc runtimeSrc;
      dummySrc = mkSource (craneLib.mkDummySrc { src = filteredSrc; }) (
        craneLib.mkDummySrc { src = runtimeSrc; }
      );
      common = {
        pname = crateName;
        version = (builtins.fromTOML (builtins.readFile "${filteredSrc}/Cargo.toml")).package.version;
        inherit src nativeBuildInputs buildInputs;
        cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = "${filteredSrc}/Cargo.lock"; };
        postUnpack = "sourceRoot+=/examples/${crateName}";
        strictDeps = true;
        doCheck = pkgs.stdenv.buildPlatform.canExecute pkgs.stdenv.hostPlatform;
        # Cargo caps dependency lints, leaving warnings fatal for our local crates.
        RUSTFLAGS = "-Dwarnings";
      };
      cargoArtifacts = craneLib.buildDepsOnly (
        (builtins.removeAttrs common [ "src" ]) // { inherit dummySrc; }
      );
      cargoChecks = {
        clippy = craneLib.cargoClippy (
          common
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          }
        );
        fmt = craneLib.cargoFmt common;
      };
    in
    craneLib.buildPackage (
      common
      // {
        inherit cargoArtifacts;
        passthru = { inherit cargoChecks; };
        inherit meta;
      }
    );

  appRustSrc =
    {
      srcDir,
      extra ? [ ],
    }:
    pkgs.lib.sources.sourceByRegex srcDir (
      [
        "Cargo.lock"
        "Cargo.toml"
        "src(/.*)?"
      ]
      ++ extra
    );

  appHostUnitDefaults = ps.hostUnitDefaults {
    environmentFileWlroots = false;
    hostGpuChecksCfg = {
      enable = false;
    };
  };

  appWebTree =
    {
      name,
      webSrc,
      webFiles,
      extraInstall ? "",
    }:
    runCommand "${name}-web" { } ''
      mkdir -p "$out/web"
      install -m644 ${appApiJsSrc} "$out/web/app-api.js"
      for f in ${lib.concatStringsSep " " webFiles}; do
        case "$f" in
          *.js)
            cat ${appApiJsSrc} ${webSrc}/"$f" > "$out/web/$f"
            ;;
          *)
            install -m644 ${webSrc}/"$f" "$out/web/$f"
            ;;
        esac
      done
      ${extraInstall}
    '';

  appWebCopyFiles =
    webTree:
    lib.map (f: {
      path = "usr/lib/jetstream/mission-control/web/${f}";
      source = "${webTree}/web/${f}";
    }) (lib.attrNames (builtins.readDir "${webTree}/web"));

  appStateStub = name: builtins.toFile "${name}-state-stub" "";

  appManifestCopyFiles =
    {
      name,
      appManifest,
      webTree,
    }:
    [
      {
        path = "usr/lib/jetstream/mission-control/manifest.json";
        source = appManifest;
      }
      {
        path = "var/lib/${name}/.keep";
        source = appStateStub name;
      }
    ]
    ++ appWebCopyFiles webTree;

in
{
  inherit
    appApiJsSrc
    buildAppCrate
    appRustSrc
    appHostUnitDefaults
    appWebTree
    appWebCopyFiles
    appStateStub
    appManifestCopyFiles
    ;
}
