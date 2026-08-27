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

  name = "hello-webui";
  jp = pe.mcJetstreamPortable;

  webTree = pe.mcWebTree {
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

  mcManifest = writeText "mission-control-manifest.json" ''
    {
      "id": "hello-webui",
      "version": "1.0.0",
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

ps.makePortableErofsImage {
  inherit name;
  unit = writeText "${name}.service" ''
    [Unit]
    Description=hello-webui (Mission Control static web UI demo)

    [Service]
    Type=oneshot
    ${jp.serviceFragment}
    ExecStart=/bin/sh -c 'echo hello-webui: static web UI demo ready'
    SyslogIdentifier=${name}
    StandardOutput=journal
    StandardError=journal
    RemainAfterExit=yes

    [Install]
    WantedBy=jetstream-apps.target
  '';
  copyFiles = pe.mcManifestCopyFiles {
    inherit name mcManifest webTree;
  };
}
