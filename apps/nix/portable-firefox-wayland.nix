# Shared Sway/Cage + Firefox portable image builder.
{
  ps,
  lib,
  stdenv,
  path,
  bash,
  replaceVars,
  writeText,
  dbus,
  pkgsStatic,
  vulkan-loader,
  compositorPkg,
  nssTools,
  p11-kit,
  adwaita-icon-theme,
  dejavu_fonts,
  name,
  description,
  stubId,
  startUrl,
  kiosk ? false,
  profileRel ? (if kiosk then "firefox-kiosk-profile" else "firefox-profile"),
  startLog,
}:

let
  inherit (ps.mkFirefoxBin { inherit lib stdenv path; }) firefox-bin;

  jp = ps.jetstreamPortableService {
    hostGpuChecksCfg = {
      gbmBackends = true;
    };
  };

  fontsConf = ps.mkFontconfigConf {
    name = "${name}-fonts.conf";
    inherit dejavu_fonts;
    cacheDir = "/run/${name}/fontconfig-cache";
  };

  clientLd = lib.makeLibraryPath [
    vulkan-loader
    stdenv.cc.cc.lib
  ];

  firefoxLoop = ps.mkFirefoxClientLoop {
    inherit nssTools p11-kit firefox-bin;
    name = "${name}-client-loop";
    firefoxCliTail =
      if kiosk then "--kiosk ${lib.escapeShellArg startUrl}" else lib.escapeShellArg startUrl;
    inherit profileRel;
  };

  compositorCmd =
    if kiosk then
      "${lib.getExe compositorPkg} -- ${firefoxLoop}"
    else
      "${lib.getExe compositorPkg} --unsupported-gpu --config \"${swayConfig}\"";

  swayConfig = writeText "${name}-sway.conf" ''
    exec ${firefoxLoop}
  '';

  clientPath = lib.makeBinPath [
    bash
    dbus
    compositorPkg
    pkgsStatic.busybox
    firefox-bin
  ];

  innerRun = replaceVars ./snippets/firefox-wayland-run.sh.in {
    bash = "${bash}";
    wlrootsPortableWaylandEnv = ps.wlrootsSwayEnvShSubst {
      bash = "${bash}";
      inherit clientLd clientPath;
      xdgRuntimeDir = "/run/${name}";
    };
    inherit fontsConf startLog;
    logTag = name;
    cursorIcons = "${adwaita-icon-theme}";
    inherit compositorCmd;
  };

  runLoopPack = ps.mkPortableRunWrapper {
    inherit name innerRun bash;
  };
  runLoop = "${runLoopPack}/run";
in
ps.makePortableErofsImage {
  inherit name;
  copyFiles =
    ps.mkPasswdGroupStubCopyFiles stubId
    ++ ps.waylandPortableBaseCopyFiles
    ++ ps.mcGpuManifestCopyFiles {
      inherit name;
      gpu = "wayland";
    };
  unit = ps.mkGpuWaylandUnit {
    inherit name jp description;
    execStart = runLoop;
    runtimeDirectory = name;
    useTty = true;
    bindHostEtc = true;
  };
  runtimeClosures = [
    runLoopPack
  ]
  ++ jp.runtimeClosures;
}
