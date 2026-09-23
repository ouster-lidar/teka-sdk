{
  ps,
  pkgs,
  lib,
  writeText,
  runCommand,
}:

let
  app = import ../../nix/app-crate.nix {
    inherit
      ps
      pkgs
      lib
      runCommand
      ;
  };

  name = "hello-slidev";

  helloSlidevSrc = app.appRustSrc { srcDir = ./.; };

  helloSlidevBin = app.buildAppCrate2nix {
    cargoNix = ./Cargo.nix;
    crateName = "hello-slidev";
    filteredSrc = helloSlidevSrc;
    nativeBuildInputs = app.appOpensslNative;
    meta = {
      mainProgram = "hello-slidev";
    };
  };

  slidevSrc = builtins.path {
    path = ./slidev;
    name = "slidev";
    filter =
      path: type:
      let
        base = baseNameOf path;
      in
      !(base == "node_modules" || base == "dist" || base == "ouster");
  };

  ousterAssets = import ./ouster-assets.nix {
    fetchurl = pkgs.buildPackages.fetchurl;
  };

  slidevSrcWithAssets = pkgs.buildPackages.runCommand "hello-slidev-slidev-src" { } ''
    mkdir -p $out/ouster
    cp -r ${slidevSrc}/. $out/
    cp ${ousterAssets.hero} $out/ouster/hero-physical-ai.png
    cp ${ousterAssets.logo} $out/ouster/logo.svg
    cp ${ousterAssets.sensorLineup} $out/ouster/sensor-lineup.png
    cp ${ousterAssets.robotics} $out/ouster/robotics.png
    cp ${ousterAssets.smartInfrastructure} $out/ouster/smart-infrastructure.png
    cp ${ousterAssets.marketsHero} $out/ouster/markets-hero.png
    cp ${ousterAssets.sensorOs1} $out/ouster/sensor-os1.png
  '';

  # Slidev is static HTML/JS/CSS — build on the build platform (x86_64 when
  # cross-compiling to aarch64) so we never pull node/icu/simdutf into the
  # target closure or remote aarch64 builders.
  slidevDist = pkgs.buildPackages.buildNpmPackage {
    pname = "hello-slidev-slidev";
    version = "1.0.0";
    src = slidevSrcWithAssets;

    npmDepsHash = "sha256-SK98Vga+VSVohVt5LLjTYw9p8ZDyjGylb6g3hGNeASo=";

    # Slidev build is custom; npm deps are fetched via npmDepsHash above.
    dontNpmBuild = true;

    buildPhase = "npm run build";
    installPhase = ''
      cp -r dist $out
      cp -r ouster $out/ouster
    '';

    preferLocalBuild = true;
  };

  jp = app.appHostUnitDefaults;

  webTree = app.appWebTree {
    inherit name;
    webSrc = ./web;
    webFiles = [
      "slides.html"
      "slides.js"
      "hello-slidev.css"
    ];
  };

  appManifest = writeText "app-manifest.json" ''
    {
      "id": "hello-slidev",
      "version": "1.0.0",
      "display_name": "Hello Slidev",
      "nav": {
        "label": "Hello Slidev",
        "path": "/app/hello-slidev",
        "section": "apps",
        "order": 16,
        "panels": [{ "label": "Slides", "panel": "slides" }]
      },
      "ui": {
        "mode": "static",
        "title": "Hello Slidev",
        "template": "slides.html",
        "styles": ["hello-slidev.css"],
        "anonymous": true
      },
      "backend": {
        "mode": "proxy",
        "runtime_listen": true,
        "health_path": "/api/v1/guest-access"
      },
      "auth": {
        "scheme": "ojs-proxy-identity",
        "version": "0.1.0"
      }
    }
  '';
in

ps.makeAppImage {
  inherit name;
  unit = writeText "${name}.service" ''
    [Unit]
    Description=hello-slidev (Slidev presentation demo)
    Documentation=file:///usr/lib/jetstream/mission-control/manifest.json
    After=mission-control.service network.target
    ${ps.appBackendUnitLines}
    Wants=network.target

    [Service]
    ${ps.appNotifyServiceLines}
    ProtectSystem=no
    ProtectHome=no
    Environment=SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt
    Environment=DECK_ROOT=/usr/lib/hello-slidev/deck
    Environment=PATH=${lib.makeBinPath [ helloSlidevBin ]}
    ${ps.appBackendServiceLines}
    StateDirectory=${name}
    StateDirectoryMode=0750
    ${jp.serviceFragment}
    ExecStart=${lib.getExe helloSlidevBin}
    SyslogIdentifier=${name}
    StandardOutput=journal
    StandardError=journal
    Restart=on-failure

    [Install]
    WantedBy=jetstream-apps.target
  '';
  copyFiles =
    app.appManifestCopyFiles {
      inherit name appManifest webTree;
    }
    ++ [
      {
        path = "usr/lib/hello-slidev/deck";
        source = slidevDist;
      }
    ];
  runtimeClosures = [ helloSlidevBin ];
}
