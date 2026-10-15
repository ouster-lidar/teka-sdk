{
  ps,
  pkgs,
  lib,
  writeText,
  runCommand,
}:

let
  app = ps.appCrate;

  name = "hello-pub";

  helloPubSrc = app.appRustSrc { srcDir = ./.; };

  helloPubBin = app.buildAppCrate {
    crateName = "hello-pub";
    filteredSrc = helloPubSrc;
    meta = {
      mainProgram = "hello-pub";
    };
  };

  jp = app.appHostUnitDefaults;

  webTree = app.appWebTree {
    inherit name;
    webSrc = ./web;
    webFiles = [
      "index.html"
      "hello-pub.html"
      "hello-pub.js"
      "hello-pub.css"
    ];
  };

  appManifest = writeText "app-manifest.json" ''
    {
      "id": "hello-pub",
      "version": "${ps.imageVersion}",
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

ps.makeAppImage {
  inherit name;
  cargoChecks = helloPubBin.passthru.cargoChecks;
  unit = writeText "${name}.service" ''
    [Unit]
    Description=hello-pub (Jetstream hello pub-sub publisher demo)
    Documentation=file:///usr/lib/jetstream/mission-control/manifest.json
    After=mission-control.service network.target
    ${ps.appBackendUnitLines}
    Wants=network.target

    [Service]
    ${ps.appNotifyServiceLines}
    ProtectSystem=no
    ProtectHome=no
    Environment=SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt
    Environment=APP_ID=hello-pub
    Environment=PATH=${lib.makeBinPath [ helloPubBin ]}
    ${ps.appBackendServiceLines}
    ${jp.serviceFragment}
    ExecStart=${lib.getExe helloPubBin}
    SyslogIdentifier=${name}
    StandardOutput=journal
    StandardError=journal
    Restart=on-failure

    [Install]
    WantedBy=jetstream-apps.target
  '';
  copyFiles = app.appManifestCopyFiles {
    inherit name appManifest webTree;
  };
  runtimeClosures = [ helloPubBin ];
}
