{
  name,
  envPrefix,
  extraBindPaths,
  extraEnvironment,
}:
{
  config,
  lib,
  teka,
  ...
}:
let
  shared = {
    runtime.network = "host";
    systemd = {
      environment = {
        "${envPrefix}_XDG_RUNTIME_DIR" = "/run/${name}";
        "${envPrefix}_WAYLAND_DISPLAY" = "wayland-1";
        "${envPrefix}_CONSOLE_LOG" = "/run/${name}/console.log";
        SSL_CERT_FILE = "/etc/ssl/certs/ca-bundle.crt";
      }
      // extraEnvironment;
      serviceConfig = {
        User = "root";
        Restart = "always";
        RestartSec = 3;
        # Bind the parent so the app can create /srv/recordings.
        BindPaths = extraBindPaths ++ [ "/srv" ];
        BindReadOnlyPaths = teka.lib.wayland.hostEtcBindPaths;
        # The backend and display units share runtime and state files.
        RuntimeDirectory = name;
        RuntimeDirectoryMode = "0700";
        StateDirectory = name;
        StateDirectoryMode = "0750";
      };
    };
  };
in
{
  services.main = lib.mkMerge [
    shared
    {
      runtime = {
        managed = true;
        # The CUDA profile keeps this service from conflicting with the compositor.
        gpu = "cuda";
      };
      systemd = {
        description = "${name} (console and display API)";
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
        environment.RUST_LOG = "info";
        serviceConfig = {
          PrivateDevices = false;
          SupplementaryGroups = "tty seat video render";
        };
      };
    }
  ];

  services.display = lib.mkMerge [
    shared
    {
      runtime.gpu = "wayland";
      systemd = {
        description = "${name} display (Sway + SimpleViz)";
        after = [
          config.services.main.unitName
          "seatd.service"
        ];
        requires = [ config.services.main.unitName ];
        wants = [ "seatd.service" ];
        wantedBy = [ "teka-apps.target" ];
        conflicts = [
          "getty@tty1.service"
          "display-manager.service"
        ];
        serviceConfig = {
          SupplementaryGroups = "tty seat";
          KillMode = "mixed";
          TimeoutStopSec = 15;
        };
      };
    }
  ];
}
