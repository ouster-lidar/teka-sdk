{
  teka,
  lib,
  stdenv,
  bash,
  writeShellScript,
  writeText,
  glmark2,
  pkgsStatic,
  sway,
  vulkan-loader,
  vulkan-tools,
  wayland,
  replaceVarsWith,
  runtimeShell,
}:

let
  display = teka.lib.wayland;
  name = teka.appId;
  mkRestartLoop =
    demo: executable: args:
    writeShellScript "${name}-${demo}-loop" ''
      while true; do
        ${executable} ${args}
        echo "${name}: ${baseNameOf executable}: exited with status $? (restarting in 1s)" >&2
        sleep 1
      done
    '';
  swayStartup = replaceVarsWith {
    name = "${name}-sway-startup";
    src = ./scripts/sway-startup.sh.in;
    isExecutable = true;
    replacements = {
      inherit runtimeShell;
      vkcubeLoop = mkRestartLoop "vkcube" "${vulkan-tools}/bin/vkcube" "--wsi wayland";
      glmark2Loop = mkRestartLoop "glmark2" "${glmark2}/bin/glmark2-wayland" "--run-forever --annotate";
    };
  };
  swayConfig = writeText "${name}-sway.conf" ''
    exec ${swayStartup}
  '';
  clientLd = lib.makeLibraryPath [
    vulkan-loader
    wayland
    stdenv.cc.cc.lib
  ];
  clientPath = lib.makeBinPath [
    pkgsStatic.busybox
    bash
    display.dbus
    glmark2
    sway
    vulkan-tools
  ];
  runLoop = replaceVarsWith {
    src = ./run.sh.in;
    isExecutable = true;
    replacements = {
      bash = "${bash}";
      wlrootsAppWaylandEnv = display.mkShellEnv { inherit clientLd clientPath; };
      inherit swayConfig;
    };
  };
in
teka.lib.mkAppImage {
  services.main = {
    runtime.gpu = "wayland";
    runtime.network = "private";
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
        User = "root";
        ExecStart = runLoop;
        Restart = "always";
        RestartSec = 3;
        BindReadOnlyPaths = [
          "-/etc/passwd:/etc/passwd"
          "-/etc/group:/etc/group"
          "-/run/seatd.sock"
        ];
        RuntimeDirectory = "%N";
        RuntimeDirectoryMode = "0700";
      };
    };
  };
}
