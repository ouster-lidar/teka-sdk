{
  ps,
  pkgs,
  lib,
  writeText,
  runCommand,
}:

let
  app = ps.appCrate;

  name = "hello-sub";

  helloSubSrc = app.appRustSrc { srcDir = ./.; };

  helloSubBin = app.buildAppCrate2nix {
    cargoNix = ./Cargo.nix;
    crateName = "hello-sub";
    filteredSrc = helloSubSrc;
    nativeBuildInputs = app.appOpensslNative;
    meta = {
      mainProgram = "hello-sub";
    };
  };

  jp = app.appHostUnitDefaults;

  webTree = app.appWebTree {
    inherit name;
    webSrc = ./web;
    webFiles = [
      "index.html"
      "hello-sub.html"
      "hello-sub.js"
      "hello-sub.css"
    ];
  };

  appManifest = writeText "app-manifest.json" ''
    {
      "id": "hello-sub",
      "version": "${ps.imageVersion}",
      "display_name": "Hello Subscriber",
      "nav": {
        "label": "Hello Sub",
        "path": "/app/hello-sub",
        "section": "apps",
        "order": 41,
        "panels": [{ "label": "Subscriber", "panel": "hello-sub" }]
      },
      "ui": {
        "mode": "static",
        "title": "Hello Subscriber",
        "template": "index.html",
        "styles": ["hello-sub.css"]
      },
      "backend": {
        "mode": "proxy",
        "runtime_listen": true,
        "health_path": "/api/v1/status",
        "card_stream_path": "/api/v1/cards/stream"
      },
      "registers": [
        { "alias": "hello-sub", "panel": "hello-sub" }
      ],
      "app_cards": [
        {
          "card_id": "hello-sub",
          "title": "Hello Sub",
          "description": "Demo event subscriber",
          "icon": "📥",
          "link_alias": "hello-sub"
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
  unit = writeText "${name}.service" ''
    [Unit]
    Description=hello-sub (Jetstream hello pub-sub subscriber demo)
    Documentation=file:///usr/lib/jetstream/mission-control/manifest.json
    After=mission-control.service network.target
    ${ps.appBackendUnitLines}
    Wants=network.target

    [Service]
    ${ps.appNotifyServiceLines}
    ProtectSystem=no
    ProtectHome=no
    StateDirectory=${name}
    StateDirectoryMode=0750
    Environment=SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt
    Environment=APP_ID=hello-sub
    Environment=MC_HELLO_PUB_APP_ID=hello-pub
    Environment=PATH=${lib.makeBinPath [ helloSubBin ]}
    ${ps.appBackendServiceLines}
    ${jp.serviceFragment}
    ExecStart=${lib.getExe helloSubBin}
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
  runtimeClosures = [ helloSubBin ];
}
