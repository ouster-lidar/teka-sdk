{ pkgs, teka }:
teka.lib.mkAppImage {
  services.main.systemd.serviceConfig = {
    Type = "oneshot";
    ExecStart = "/bin/true";
    RemainAfterExit = true;
  };
  image.packages = [
    (pkgs.linkFarm "hello-webui-web" { "${teka.appDir}/web" = ./web; })
    pkgs.teka-app-api-js
  ];
}
