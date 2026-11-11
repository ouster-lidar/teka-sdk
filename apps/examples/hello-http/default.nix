{
  image =
    {
      ps,
      python3Minimal,
      replaceVars,
    }:

    let
      app = ps.appCrate;

      inherit (ps) tekaRunner;

      name = "hello-http";
      port = 8000;

      jp = ps.hostUnitDefaults {
        hostGpuChecksCfg = {
          enable = false;
        };
      };

      serverPy = replaceVars ./server.py.in {
        port = toString port;
      };

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

      appManifest = replaceVars ./manifest.json {
        imageVersion = ps.imageVersion;
        port = toString port;
      };
    in

    ps.makeAppImage {
      inherit name;
      unit = replaceVars ./hello-http.service {
        serviceFragment = jp.serviceFragment;
        inherit tekaRunner;
        port = toString port;
        inherit python3Minimal;
      };
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
        tekaRunner
      ];
    };
}
