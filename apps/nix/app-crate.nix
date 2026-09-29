{
  ps,
  pkgs,
  lib,
  runCommand,
  appApiJs ? ../web/app-api.js,
  appRuntimeSrc ? ../lib/app-runtime,
}:

let
  appApiJsSrc =
    if lib.isDerivation appApiJs then
      "${appApiJs}/mc-app-api.js"
    else
      builtins.path {
        path = appApiJs;
        name = "app-api.js";
      };

  appOpensslNative = with pkgs; [
    pkg-config
    openssl
  ];

  appRuntimeSrcFilter =
    if lib.isDerivation appRuntimeSrc then appRuntimeSrc else lib.cleanSource appRuntimeSrc;

  strictRustWarnings = attrs: {
    extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "-Dwarnings" ];
    capLints = "forbid";
  };

  appCrateOverridesFor =
    {
      crateName,
      filteredSrc ? null,
      targetPkgs ? pkgs,
    }:
    targetPkgs.defaultCrateOverrides
    // {
      app-runtime =
        attrs:
        strictRustWarnings attrs
        // {
          src = appRuntimeSrcFilter;
        };
      jetstream-host = strictRustWarnings;
      ${crateName} =
        attrs:
        strictRustWarnings attrs
        // {
          src = if filteredSrc != null then filteredSrc else attrs.src;
          preBuild = (attrs.preBuild or "") + ''
            export CARGO_CRATE_NAME="${attrs.libName or (lib.replaceStrings [ "-" ] [ "_" ] crateName)}"
          '';
        };
    };

  rootCrateBuildFor =
    crateOverridesForPkgs:
    import ./cross-crate2nix.nix {
      inherit lib pkgs crateOverridesForPkgs;
    };

  buildCrate2nix =
    {
      cargoNix,
      packageId,
      crateOverridesForPkgs,
      rootFeatures ? [ "default" ],
      nativeBuildInputs ? [ ],
      meta ? { },
    }:
    (rootCrateBuildFor crateOverridesForPkgs {
      inherit cargoNix packageId rootFeatures;
    }).overrideAttrs
      (old: {
        nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ nativeBuildInputs;
        meta = (old.meta or { }) // meta;
      });

  buildAppCrate2nix =
    {
      cargoNix,
      crateName,
      filteredSrc ? null,
      rootFeatures ? [ "default" ],
      nativeBuildInputs ? [ ],
      meta ? { },
    }:
    buildCrate2nix {
      inherit
        cargoNix
        nativeBuildInputs
        meta
        rootFeatures
        ;
      packageId = crateName;
      crateOverridesForPkgs =
        targetPkgs:
        appCrateOverridesFor {
          inherit crateName filteredSrc targetPkgs;
        };
    };

  appRustSrc =
    {
      srcDir,
      extra ? [ ],
    }:
    pkgs.lib.sources.sourceByRegex srcDir (
      [
        "Cargo.lock"
        "Cargo.toml"
        "Cargo.nix"
        "default.nix"
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
      install -m644 ${appApiJsSrc} "$out/web/mc-app-api.js"
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
    appOpensslNative
    buildAppCrate2nix
    strictRustWarnings
    appRustSrc
    appHostUnitDefaults
    appWebTree
    appWebCopyFiles
    appStateStub
    appManifestCopyFiles
    ;
}
