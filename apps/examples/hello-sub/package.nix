{ pkgs, teka }:
let
  program = teka.lib.buildRustApp { srcDir = ./.; };
in
teka.lib.mkAppImage {
  services.main = {
    inherit program;
    runtime.managed = true;
    runtime.network = "host";
    systemd = {
      after = [ "network.target" ];
      wants = [ "network.target" ];
      environment.TEKA_HELLO_PUB_APP_ID = "hello-pub";
      serviceConfig = {
        ProtectSystem = false;
        ProtectHome = false;
        StateDirectory = "%N";
        StateDirectoryMode = "0750";
        Restart = "on-failure";
      };
    };
  };
  image.packages = [
    (pkgs.linkFarm "hello-sub-web" { "${teka.appDir}/web" = ./web; })
    pkgs.teka-app-api-js
  ];
}
