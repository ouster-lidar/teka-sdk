{
  ps,
  pkgs,
  lib,
  runCommand,
  mcAppApiJs ? ../web/mc-app-api.js,
}:

let
  mcAppApiJsSrc = builtins.path {
    path = mcAppApiJs;
    name = "mc-app-api.js";
  };

  mcOpensslNative = with pkgs; [
    pkg-config
    openssl
  ];

  mcCrateOverridesFor =
    {
      crateName,
      filteredSrc ? null,
      targetPkgs ? pkgs,
    }:
    targetPkgs.defaultCrateOverrides
    // {
      ${crateName} = attrs: {
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
      nativeBuildInputs ? [ ],
      meta ? { },
    }:
    (rootCrateBuildFor crateOverridesForPkgs {
      inherit cargoNix packageId;
    }).overrideAttrs
      (old: {
        nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ nativeBuildInputs;
        meta = (old.meta or { }) // meta;
      });

  buildMcCrate2nix =
    {
      cargoNix,
      crateName,
      filteredSrc ? null,
      nativeBuildInputs ? [ ],
      meta ? { },
    }:
    buildCrate2nix {
      inherit cargoNix nativeBuildInputs meta;
      packageId = crateName;
      crateOverridesForPkgs =
        targetPkgs:
        mcCrateOverridesFor {
          inherit crateName filteredSrc targetPkgs;
        };
    };

  mcRustSrc =
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

  mcJetstreamPortable = ps.jetstreamPortableService {
    environmentFileWlroots = false;
    hostGpuChecksCfg = {
      enable = false;
    };
  };

  mcWebTree =
    {
      name,
      webSrc,
      webFiles,
      extraInstall ? "",
    }:
    runCommand "${name}-web" { } ''
      mkdir -p "$out/web"
      install -m644 ${mcAppApiJsSrc} "$out/web/mc-app-api.js"
      for f in ${lib.concatStringsSep " " webFiles}; do
        case "$f" in
          *.js)
            cat ${mcAppApiJsSrc} ${webSrc}/"$f" > "$out/web/$f"
            ;;
          *)
            install -m644 ${webSrc}/"$f" "$out/web/$f"
            ;;
        esac
      done
      ${extraInstall}
    '';

  mcWebCopyFiles =
    webTree:
    lib.map (f: {
      path = "usr/lib/jetstream/mission-control/web/${f}";
      source = "${webTree}/web/${f}";
    }) (lib.attrNames (builtins.readDir "${webTree}/web"));

  mcStateStub = name: builtins.toFile "${name}-state-stub" "";

  mcManifestCopyFiles =
    {
      name,
      mcManifest,
      webTree,
    }:
    [
      {
        path = "usr/lib/jetstream/mission-control/manifest.json";
        source = mcManifest;
      }
      {
        path = "var/lib/${name}/.keep";
        source = mcStateStub name;
      }
    ]
    ++ mcWebCopyFiles webTree;

in
{
  inherit
    mcAppApiJsSrc
    mcOpensslNative
    buildMcCrate2nix
    mcRustSrc
    mcJetstreamPortable
    mcWebTree
    mcWebCopyFiles
    mcStateStub
    mcManifestCopyFiles
    ;
}
