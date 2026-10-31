{
  ps,
  pkgs,
  lib,
  writeText,
  runCommand,
}:

let
  app = ps.appCrate;

  name = "hello-webui";
  jp = app.appHostUnitDefaults;

  webTree = app.appWebTree {
    inherit name;
    webSrc = ./web;
    webFiles = [
      "demo.html"
      "demo.js"
      "game.html"
      "game.js"
      "hello-webui.css"
    ];
  };

  appManifest = writeText "app-manifest.json" ''
    {
      "id": "hello-webui",
      "version": "${ps.imageVersion}",
      "display_name": "Hello WebUI",
      "nav": {
        "label": "Hello WebUI",
        "path": "/app/hello-webui",
        "section": "apps",
        "order": 15,
        "panels": [
          { "label": "Demo", "panel": "demo" },
          { "label": "Snake", "panel": "game" }
        ]
      },
      "ui": {
        "mode": "static",
        "title": "Hello WebUI",
        "template": "demo.html",
        "styles": ["hello-webui.css"]
      },
      "backend": { "mode": "none" },
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
    Description=hello-webui (static web UI demo)

    [Service]
    Type=oneshot
    ${jp.serviceFragment}
    ExecStart=/bin/sh -c 'echo hello-webui: static web UI demo ready'
    SyslogIdentifier=${name}
    StandardOutput=journal
    StandardError=journal
    RemainAfterExit=yes

    [Install]
    WantedBy=teka-apps.target
  '';
  copyFiles = app.appManifestCopyFiles {
    inherit name appManifest webTree;
  };
}
