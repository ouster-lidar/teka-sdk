# Shared Sway/Cage + Firefox app image.
{
  teka,
  imports,
  lib,
  stdenv,
  firefox-bin,
  bash,
  replaceVarsWith,
  writeText,
  callPackage,
  pkgsStatic,
  vulkan-loader,
  sway,
  cage,
  nssTools,
  p11-kit,
  adwaita-icon-theme,
}:
teka.lib.mkAppImage (
  { config, ... }:
  let
    name = teka.appId;
    display = teka.lib.wayland;
    inherit (display) fontsConf;

    kiosk = config.mode == "kiosk";
    compositorPkg = if kiosk then cage else sway;
    compositorName = if kiosk then "Cage" else "Sway";
    browserName = if kiosk then "Firefox kiosk" else "Firefox";
    url = lib.escapeShellArg config.startUrl;

    clientLd = lib.makeLibraryPath [
      vulkan-loader
      stdenv.cc.cc.lib
    ];
    clientPath = lib.makeBinPath [
      bash
      display.dbus
      compositorPkg
      pkgsStatic.busybox
      firefox-bin
    ];

    firefoxLoop = callPackage ./client-loop.nix {
      inherit nssTools p11-kit firefox-bin;
      name = "${name}-client-loop";
      firefoxCliTail = if kiosk then "--kiosk ${url}" else url;
      profileRel = if kiosk then "firefox-kiosk-profile" else "firefox-profile";
    };

    swayConfig = writeText "${name}-sway.conf" ''
      exec ${firefoxLoop}
    '';

    runLoop = replaceVarsWith {
      src = ./firefox-wayland-run.sh.in;
      isExecutable = true;
      replacements = {
        bash = "${bash}";
        wlrootsAppWaylandEnv = display.mkShellEnv { inherit clientLd clientPath; };
        inherit fontsConf;
        startLog = "starting ${compositorName} (wlroots) + ${browserName} on DRM with NVIDIA clients";
        logTag = name;
        cursorIcons = "${adwaita-icon-theme}";
        compositorCmd =
          if kiosk then
            "${lib.getExe compositorPkg} -- ${firefoxLoop}"
          else
            "${lib.getExe compositorPkg} --unsupported-gpu --config \"${swayConfig}\"";
      };
    };

  in
  {
    options = {
      mode = lib.mkOption {
        type = lib.types.enum [
          "browser"
          "kiosk"
        ];
        description = "Run Firefox in Sway or in kiosk mode under Cage.";
      };
      startUrl = lib.mkOption {
        type = lib.types.str;
        description = "Page Firefox opens at startup.";
      };
    };
    config = {
      services.main = {
        runtime.gpu = "wayland";
        runtime.network = "host";
        systemd = {
          description = "${name} (${compositorName} + ${browserName} on Wayland)";
          after = [
            "systemd-user-sessions.service"
            "network.target"
            "network-online.target"
            "time-sync.target"
            "seatd.service"
          ];
          wants = [
            "network-online.target"
            "time-sync.target"
            "seatd.service"
          ];
          conflicts = [
            "getty@tty1.service"
            "display-manager.service"
          ];
          serviceConfig = {
            User = "root";
            ExecStart = runLoop;
            Restart = "always";
            RestartSec = 3;
            StandardInput = "tty";
            StandardOutput = "journal";
            TTYPath = "/dev/tty1";
            TTYReset = true;
            TTYVHangup = true;
            TTYVTDisallocate = true;
            BindReadOnlyPaths = display.hostEtcBindPaths;
            RuntimeDirectory = "%N";
            RuntimeDirectoryMode = "0700";
          };
        };
      };
    };
    inherit imports;
  }
)
