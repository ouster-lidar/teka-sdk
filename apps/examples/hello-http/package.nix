{
  lib,
  pkgs,
  teka,
}:
let
  port = toString (
    lib.toInt (lib.removePrefix "http://127.0.0.1:" (lib.importJSON ./manifest.json).backend.upstream)
  );
in
teka.lib.mkAppImage {
  services.main = {
    runtime.managed = true;
    runtime.network = "host";
    systemd.serviceConfig = {
      ExecStart = "${lib.getExe teka.lib.tekaRunner} ${teka.appId} tcp ${port} ${pkgs.python3Minimal}/bin/python3 ${./server.py} ${port}";
      Restart = "on-failure";
    };
  };
  image.packages = [
    (pkgs.buildPackages.runCommand "hello-http-web" { } ''
      web="$out/${teka.appDir}/web"
      install -Dm644 -t "$web" ${./web}/{hello-http.html,hello-http.js}
      substituteInPlace "$web/hello-http.js" --replace-fail "__HELLO_HTTP_PORT__" "${port}"
    '')
    pkgs.teka-app-api-js
  ];
}
