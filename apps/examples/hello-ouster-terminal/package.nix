{
  teka,
  lib,
  linkFarm,
  buildEnv,
  stdenv,
  bash,
  runCommand,
  replaceVars,
  writeText,
  writeShellScript,
  pkgsStatic,
  vulkan-loader,
  sway,
  wayland,
  glfw,
  libxkbcommon,
  libdecor,
  foot,
  cacert,
  python3,
  python3Packages,
  bat,
  replaceVarsWith,
  runtimeShell,
}:

let
  display = teka.lib.wayland;
  name = "hello-ouster-terminal";

  inherit (teka.lib) tekaRunner;

  ousterUdpFirewallRange = "7000-8000";
  ousterLidarDataPort = 7602;
  ousterImuDataPort = 7603;

  # Busybox first so POSIX staples (ls, cp, grep, …) resolve before other bin/ (debug shells).
  serviceBinPath = lib.makeBinPath [
    pkgsStatic.busybox
    bash
    bat
    display.dbus
    foot
    sway
    python3Packages.ouster-sdk
    python3
  ];

  # In-kiosk README (bat in foot). See README.md: # headings (not
  # underline titles), inline `command` OK, no triple-backtick fences (indent examples),
  # no bold; cross-ref sway.md / viz.md for layout and viz details.
  kioskReadme = replaceVars ./docs/kiosk.md.in {
    inherit ousterUdpFirewallRange;
    ousterLidarDataPort = toString ousterLidarDataPort;
    ousterImuDataPort = toString ousterImuDataPort;
  };

  kioskVizMd = replaceVars ./docs/viz.md.in {
    ousterLidarDataPort = toString ousterLidarDataPort;
    ousterImuDataPort = toString ousterImuDataPort;
  };

  directionsLaunch = writeShellScript "${name}-directions" ''
    if [[ -r "$HOME/docs/README.md" ]]; then
      ${lib.getExe bat} --no-paging -p "$HOME/docs/README.md"
    fi
    printf '\n\n'
    exec ${bash}/bin/bash -i
  '';

  # Host EGL/GLVND/GBM first (/run/opengl-driver + generator); DDI carries GLFW/Wayland only.
  clientLd = lib.makeLibraryPath [
    vulkan-loader
    wayland
    glfw
    libxkbcommon
    libdecor
    stdenv.cc.cc.lib
  ];

  ousterVizLoop = replaceVarsWith {
    name = "${name}-ouster-viz-loop";
    src = ./scripts/viz-loop.sh.in;
    isExecutable = true;
    replacements = {
      inherit runtimeShell;
      caBundlePath = "${cacert}/etc/ssl/certs/ca-bundle.crt";
      logTag = name;
      pickOusterSensorPy = "${./pick_ouster_sensor.py}";
      lidarPort = toString ousterLidarDataPort;
      imuPort = toString ousterImuDataPort;
      swayBin = sway;
    };
  };

  swayAutostart = replaceVarsWith {
    name = "${name}-sway-autostart";
    src = ./scripts/sway-autostart.sh.in;
    isExecutable = true;
    replacements = {
      inherit
        runtimeShell
        sway
        footExe
        directionsLaunch
        ousterVizLoop
        ;
      footIni = "${footIni}";
    };
  };

  footExe = lib.getExe foot;
  # Shared by sway `exec` and Mod4+Shift+Return so every Foot uses the same font as viz.
  footIni = ./config/foot.ini;

  swayConfig = replaceVars ./config/sway.conf.in {
    inherit footExe bash swayAutostart;
    footIni = "${footIni}";
  };

  ddiBinPathExport = "export PATH=\"/bin:/usr/bin\${PATH:+:$PATH}\"";

  innerRun = replaceVars ./run-wayland.sh.in {
    bash = "${bash}";
    inherit clientLd;
    ldPreloadExport = display.mesaLlvmPreload;
    clientPath = serviceBinPath;
    fontsConf = "${display.fontsConf}";
    xdgRuntimeDir = "/run/${teka.appId}";
    swayConfig = "${swayConfig}";
    bashrc = "${./config/bashrc}";
  };

  runLoopPack = runCommand "${name}-run-pkg" { } ''
    mkdir -p "$out/docs"
    install -m755 ${innerRun} "$out/run"
    install -m444 ${kioskReadme} "$out/docs/README.md"
    install -m444 ${./docs/sway.md} "$out/docs/sway.md"
    install -m444 ${kioskVizMd} "$out/docs/viz.md"
  '';
in
teka.lib.mkAppImage {
  imports = [ ./configuration.nix ];

  services.main.systemd.serviceConfig.ExecStart =
    "${tekaRunner}/bin/teka-runner ${teka.appId} udp ${ousterUdpFirewallRange} ${runLoopPack}/run";

  image.packages = [
    (linkFarm "${name}-root" {
      "usr/lib/extension-release.d/extension-release.hello-ouster-terminal" = ./config/extension-release;
      "etc/profile.d/zz-${name}-binpath.sh" = writeText "${name}-profile-d-binpath.sh" ddiBinPathExport;
      "etc/bash.bashrc" = writeText "${name}-etc-bash.bashrc" ddiBinPathExport;
      "root/.bashrc" = replaceVars ./config/root-bashrc.sh.in { inherit ddiBinPathExport; };
    })
    (buildEnv {
      name = "${name}-shell-tools";
      paths = [ (lib.getBin bash) ];
      pathsToLink = [ "/bin" ];
    })
  ];
}
