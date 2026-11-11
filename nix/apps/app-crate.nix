{
  ps,
  pkgs,
  lib,
  runCommand,
  craneLib,
  advisoryDb ? null,
  appApiJs ? ./web/app-api.js,
  tekaSrc ? pkgs.buildPackages.callPackage ../packages/teka-src.nix { inherit craneLib; },
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

  prepareAppSource = pkgs.buildPackages.callPackage ./app-source.nix {
    inherit tekaSrc craneLib;
  };

  buildAppCrate =
    {
      src,
      dummySrc,
      cargoRoot,
      cargoToml,
      cargoLock,
      nativeBuildInputs ? [ ],
      buildInputs ? [ ],
      meta ? { },
    }:
    let
      manifest = builtins.fromTOML (builtins.readFile cargoToml);
      common = {
        pname = manifest.package.name;
        version = manifest.package.version;
        inherit src nativeBuildInputs buildInputs;
        cargoVendorDir = craneLib.vendorCargoDeps { inherit cargoLock; };
        postUnpack = "sourceRoot+=/${lib.escapeShellArg cargoRoot}";
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
          src = builtins.dirOf cargoLock;
          advisory-db = advisoryDb;
          pname = manifest.package.name;
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
    prepareAppSource
    buildAppCrate
    appRustSrc
    appHostUnitDefaults
    appWebTree
    appWebCopyFiles
    appStateStub
    appManifestCopyFiles
    ;
}
