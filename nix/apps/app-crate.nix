{
  ps,
  pkgs,
  lib,
  runCommand,
  craneLib,
  advisoryDb ? null,
  appApiJs ? ./web/app-api.js,
  tekaSrc ? pkgs.buildPackages.callPackage ../packages/teka-src.nix { },
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
      sdkSrc = if lib.isDerivation tekaSrc then tekaSrc else lib.cleanSource tekaSrc;
      # Preserve Cargo's relative path dependency, including downstream overrides.
      mkSource =
        appSrc: sdkSrc:
        pkgs.buildPackages.runCommand "${crateName}-source" { } ''
          mkdir -p "$out/apps/examples/${crateName}" "$out/crates"
          cp -r ${appSrc}/. "$out/apps/examples/${crateName}/"
          cp -r ${sdkSrc} "$out/crates/teka"
        '';
      src = mkSource filteredSrc sdkSrc;
      dummySrc = mkSource (craneLib.mkDummySrc { src = filteredSrc; }) (
        craneLib.mkDummySrc { src = sdkSrc; }
      );
      common = {
        pname = crateName;
        version = (builtins.fromTOML (builtins.readFile "${filteredSrc}/Cargo.toml")).package.version;
        inherit src nativeBuildInputs buildInputs;
        cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = "${filteredSrc}/Cargo.lock"; };
        postUnpack = "sourceRoot+=/apps/examples/${crateName}";
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
      }
      // lib.optionalAttrs (advisoryDb != null) {
        audit = craneLib.cargoAudit {
          src = filteredSrc;
          advisory-db = advisoryDb;
          pname = crateName;
          version = common.version;
        };
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

  # Keep web assets out of evaluation so other platforms need no native builder.
  appWebCopyFiles = webTree: [
    {
      path = "usr/lib/teka/mission-control/web";
      source = "${webTree}/web/.";
    }
  ];

  appStateStub = name: builtins.toFile "${name}-state-stub" "";

  appManifestCopyFiles =
    {
      name,
      appManifest,
      webTree,
    }:
    [
      {
        path = "usr/lib/teka/mission-control/manifest.json";
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
