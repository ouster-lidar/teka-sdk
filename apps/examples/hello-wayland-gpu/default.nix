{
  ps,
  lib,
  stdenv,
  bash,
  replaceVars,
  writeText,
  writeShellScript,
  dbus,
  glmark2,
  pkgsStatic,
  sway,
  vulkan-loader,
  vulkan-tools,
  wayland,
}:

let
  name = "hello-wayland-gpu";
  jp = ps.hostUnitDefaults {
    hostGpuChecksCfg = {
      enable = true;
      gbmBackends = true;
    };
  };
  vkcubeLoop = ps.mkRestartLoop {
    name = "hello-wayland-gpu-vkcube-loop";
    cmd = "${vulkan-tools}/bin/vkcube --wsi wayland";
    label = "hello-wayland-gpu: vkcube";
  };
  glmark2Loop = ps.mkRestartLoop {
    name = "hello-wayland-gpu-glmark2-loop";
    cmd = "${glmark2}/bin/glmark2-wayland --run-forever --annotate";
    label = "hello-wayland-gpu: glmark2-wayland";
  };
  swayStartup = writeShellScript "hello-wayland-gpu-sway-startup" ''
    set -euo pipefail
    if [[ "''${WLR_BACKENDS:-}" == headless* ]]; then
      for _ in $(seq 1 100); do
        outputs="$(swaymsg -t get_outputs 2>/dev/null || true)"
        if [[ "$outputs" == *'"active": true'* ]]; then
          swaymsg workspace 1 >/dev/null 2>&1 || true
          workspaces="$(swaymsg -t get_workspaces 2>/dev/null || true)"
          [[ "$workspaces" == *'"name"'* ]] && break
        fi
        sleep 0.2
      done
      sleep 1
    fi
    swaymsg exec "${vkcubeLoop}" >/dev/null 2>&1 || true
    exec swaymsg exec "${glmark2Loop}" >/dev/null 2>&1 || true
  '';
  swayConfig = writeText "hello-wayland-gpu-sway.conf" ''
    xwayland disable
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
    dbus
    glmark2
    sway
    vulkan-tools
  ];
  innerRun = replaceVars ./run.sh.in {
    bash = "${bash}";
    wlrootsAppWaylandEnv = ps.wlrootsSwayEnvShSubst {
      bash = "${bash}";
      inherit clientLd clientPath;
      xdgRuntimeDir = "/run/hello-wayland-gpu";
    };
    inherit swayConfig;
  };
  runLoopPack = ps.mkAppRunWrapper {
    inherit name innerRun bash;
  };
  runLoop = "${runLoopPack}/run";
in
ps.makeAppImage {
  inherit name;
  copyFiles =
    ps.mkPasswdGroupStubCopyFiles "hwl"
    ++ ps.waylandAppBaseCopyFiles
    ++ ps.appGpuManifestCopyFiles {
      inherit name;
      gpu = "wayland";
    };
  unit = ps.mkGpuWaylandUnit {
    inherit name jp;
    description = "hello-wayland-gpu (Sway + Vulkan/OpenGL Wayland demos)";
    execStart = runLoop;
    runtimeDirectory = name;
    useTty = false;
    bindHostEtc = false;
  };
  runtimeClosures = [
    runLoopPack
  ]
  ++ jp.runtimeClosures;
}
