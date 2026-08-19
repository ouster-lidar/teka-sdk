{
  ps,
  pkgs,
  lib,
  writeText,
  runCommand,
}:

let
  pe = import ../../nix/portable-app.nix {
    inherit
      ps
      pkgs
      lib
      runCommand
      ;
  };

  name = "hello-pub";

  helloPubSrc = pe.mcRustSrc { srcDir = ./.; };

  helloPubBin = pe.buildMcCrate2nix {
    cargoNix = ./Cargo.nix;
    crateName = "hello-pub";
    filteredSrc = helloPubSrc;
    nativeBuildInputs = pe.mcOpensslNative;
    meta = {
      mainProgram = "hello-pub";
    };
  };

  jp = pe.mcJetstreamPortable;

  webTree = pe.mcWebTree {
    inherit name;
    webSrc = ./web;
    webFiles = [
      "index.html"
      "hello-pub.html"
      "hello-pub.js"
      "hello-pub.css"
    ];
  };

  mcManifest = writeText "mission-control-manifest.json" ''
    {
      "id": "hello-pub",
      "version": "1.0.0",
      "display_name": "Hello Publisher",
      "nav": {
        "label": "Hello Pub",
        "path": "/app/hello-pub",
        "section": "apps",
        "order": 40,
        "panels": [{ "label": "Publisher", "panel": "hello-pub" }]
      },
      "ui": {
        "mode": "static",
        "title": "Hello Publisher",
        "template": "index.html",
        "styles": ["hello-pub.css"]
      },
      "backend": {
        "mode": "proxy",
        "runtime_listen": true,
        "health_path": "/api/v1/state",
        "card_stream_path": "/api/v1/cards/stream"
      },
      "registers": [
        { "alias": "hello-pub", "panel": "hello-pub" }
      ],
      "app_cards": [
        {
          "card_id": "hello-pub",
          "title": "Hello Pub",
          "description": "Demo event publisher",
          "icon": "📤",
          "link_alias": "hello-pub"
        }
      ],
      "auth": {
        "scheme": "ojs-proxy-identity",
        "version": "0.1.0"
      }
    }
  '';
in

ps.makePortableErofsImage {
  inherit name;
  unit = writeText "${name}.service" ''
    [Unit]
    Description=hello-pub (Jetstream hello pub-sub publisher demo)
    Documentation=file:///usr/lib/jetstream/mission-control/manifest.json
    After=mission-control.service network.target
    ${ps.jetstreamMcPortableBackendUnitLines}
    Wants=network.target

    [Service]
    ${ps.mcPortableNotifyServiceLines}
    ProtectSystem=no
    ProtectHome=no
    Environment=SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt
    Environment=APP_ID=hello-pub
    Environment=PATH=${lib.makeBinPath [ helloPubBin ]}
    ${ps.jetstreamMcPortableBackendServiceLines}
    ${jp.serviceFragment}
    ExecStart=${lib.getExe helloPubBin}
    SyslogIdentifier=${name}
    StandardOutput=journal
    StandardError=journal
    Restart=on-failure

    [Install]
    WantedBy=jetstream-apps.target
  '';
  copyFiles = pe.mcManifestCopyFiles {
    inherit name mcManifest webTree;
  };
  runtimeClosures = [ helloPubBin ];
}
