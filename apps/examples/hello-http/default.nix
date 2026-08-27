{
  ps,
  writeText,
  python3Minimal,
  firewalld,
}:

let
  inherit (ps.callPortableFirewalld firewalld) firewallCmd firewalldCli;

  name = "hello-http";
  port = 8000;

  jp = ps.jetstreamPortableService {
    hostGpuChecksCfg = {
      enable = false;
    };
  };

  serverPy = writeText "server.py" ''
    from http.server import HTTPServer, BaseHTTPRequestHandler

    _BODY = (
        b"<!DOCTYPE html><html><head>"
        b'<meta charset="utf-8">'
        b'<link rel="stylesheet" href="/static/css/app.css">'
        b"<title>Hello HTTP</title></head>"
        b'<body style="background:transparent;min-height:auto">'
        b'<div class="main" style="padding:1.5rem">'
        b'<div class="stat-card">'
        b"<h2>Hello world</h2>"
        b"<p>Proxy example app using Jetstream shared styles.</p>"
        b"</div></div></body></html>\n"
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

  mcManifest = writeText "mission-control-manifest.json" ''
    {
      "id": "hello-http",
      "version": "1.0.0",
      "display_name": "Hello HTTP",
      "nav": {
        "label": "Hello HTTP",
        "path": "/app/hello-http",
        "section": "apps",
        "order": 10
      },
      "ui": { "mode": "proxy", "title": "Hello HTTP" },
      "backend": { "mode": "proxy", "upstream": "http://127.0.0.1:${toString port}" },
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
    Description=hello-http (python http.server)
    After=firewalld.service
    Wants=firewalld.service

    [Service]
    Type=simple
    ${jp.serviceFragment}
    ExecStartPre=${firewallCmd} --add-port=${toString port}/tcp
    ExecStart=${python3Minimal}/bin/python3 /usr/lib/hello-http/server.py
    ExecStopPost=${firewallCmd} --remove-port=${toString port}/tcp
    SyslogIdentifier=${name}
    StandardOutput=journal
    StandardError=journal
    BindPaths=-/run/dbus/system_bus_socket
    Restart=on-failure

    [Install]
    WantedBy=jetstream-apps.target
  '';
  copyFiles = [
    {
      path = "usr/lib/hello-http/server.py";
      source = serverPy;
    }
    {
      path = "usr/lib/jetstream/mission-control/manifest.json";
      source = mcManifest;
    }
  ];
  runtimeClosures = [
    python3Minimal
    firewalldCli
  ];
}
