{ pkgs, teka, ... }:
{
  services.main = {
    # teka-runner reports readiness after leasing the sensor's UDP ports.
    runtime.managed = true;
    runtime.network = "host";
    runtime.gpu = "wayland";
    systemd = {
      after = [
        "systemd-user-sessions.service"
        "network.target"
        "network-online.target"
        "seatd.service"
      ];
      wants = [
        "network-online.target"
        "seatd.service"
      ];
      conflicts = [
        "getty@tty1.service"
        "display-manager.service"
      ];
      serviceConfig = {
        DynamicUser = true;
        Restart = "always";
        RestartSec = 3;
        StandardInput = "tty";
        StandardOutput = "journal";
        TTYPath = "/dev/tty1";
        TTYReset = true;
        TTYVHangup = true;
        TTYVTDisallocate = true;
        BindReadOnlyPaths = teka.lib.wayland.hostEtcBindPaths;
        RuntimeDirectory = "%N";
        RuntimeDirectoryMode = "0700";
        SupplementaryGroups = "tty seat video render";
      };
    };
  };

  image.packages = [
    (pkgs.linkFarm "hello-ouster-terminal-web" { "${teka.appDir}/web" = ./web; })
    teka.lib.webApi
  ];
}
