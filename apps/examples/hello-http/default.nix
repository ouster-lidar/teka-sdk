{
  ps,
  pkgs,
  writeText,
  python3Minimal,
}:

let
  app = ps.appCrate;

  appRuntime = pkgs.callPackage ../../nix/app-runtime.nix { };

  name = "hello-http";
  port = 8000;

  jp = ps.hostUnitDefaults {
    hostGpuChecksCfg = {
      enable = false;
    };
  };

  serverPy = writeText "server.py" ''
    from http.server import HTTPServer, BaseHTTPRequestHandler

    _BODY = (
        b"<!DOCTYPE html><html><head>"
        b'<meta charset="utf-8">'
        b"<title>Hello HTTP</title>"
        b"</head>"
        b"<body><main>"
        b"<h1>Hello world</h1>"
        b"<p>Python HTTP server reached on the app's direct TCP port.</p>"
        b"</main></body></html>\n"
    )

    class Handler(BaseHTTPRequestHandler):
        def _log_headers(self):
            for line in str(self.headers).strip().splitlines():
                print(line, flush=True)

        def do_GET(self):
            self._log_headers()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(_BODY)))
            self.end_headers()
            self.wfile.write(_BODY)

        def do_HEAD(self):
            self._log_headers()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(_BODY)))
            self.end_headers()

    HTTPServer(("0.0.0.0", ${toString port}), Handler).serve_forever()
  '';

  webTree = app.appWebTree {
    inherit name;
    webSrc = ./web;
    webFiles = [
      "hello-http.html"
      "hello-http.js"
    ];
    extraInstall = ''
      substituteInPlace "$out/web/hello-http.js" --replace-fail "__HELLO_HTTP_PORT__" "${toString port}"
    '';
  };

  appManifest = writeText "app-manifest.json" ''
    {
      "id": "hello-http",
      "version": "${ps.imageVersion}",
      "display_name": "Hello HTTP",
      "nav": {
        "label": "Hello HTTP",
        "path": "/app/hello-http",
        "section": "apps",
        "order": 10,
        "panels": [{ "label": "Hello HTTP", "panel": "hello-http" }]
      },
      "ui": { "mode": "static", "title": "Hello HTTP", "template": "hello-http.html" },
      "backend": { "mode": "proxy", "upstream": "http://127.0.0.1:${toString port}" },
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
    Description=hello-http (python http.server)
    After=mission-control.service
    Wants=mission-control.service

    [Service]
    Type=notify
    ${jp.serviceFragment}
    ExecStart=${appRuntime}/bin/app-runtime-process ${name} tcp ${toString port} ${python3Minimal}/bin/python3 /usr/lib/hello-http/server.py
    SyslogIdentifier=${name}
    StandardOutput=journal
    StandardError=journal
    Restart=on-failure
    TimeoutStartSec=90

    [Install]
    WantedBy=teka-apps.target
  '';
  copyFiles =
    app.appManifestCopyFiles {
      inherit name appManifest webTree;
    }
    ++ [
      {
        path = "usr/lib/hello-http/server.py";
        source = serverPy;
      }
    ];
  runtimeClosures = [
    python3Minimal
    appRuntime
  ];
}
