{ pkgs, teka }:
let
  program = teka.lib.buildRustApp { srcDir = ./.; };
in
teka.lib.mkAppImage {
  services.main = {
    inherit program;
    runtime.managed = true;
    systemd = {
      after = [ "network.target" ];
      wants = [ "network.target" ];
      serviceConfig = {
        ProtectSystem = false;
        ProtectHome = false;
        Restart = "on-failure";
      };
    };
  };
  image.packages = [
    (pkgs.linkFarm "hello-pub-web" { "${teka.appDir}/web" = ./web; })
    pkgs.teka-app-api-js
  ];
}
