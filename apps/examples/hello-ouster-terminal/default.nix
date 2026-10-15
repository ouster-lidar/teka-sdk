# App image DDI: Sway Wayland kiosk + foot, running ouster-cli viz against nixpkgs
# GLFW / python3Packages.glfw on native Wayland (GL prefix resolver, MESA shader
# cache, /bin bash+busybox, systemd-run README).
{
  ps,
  pkgs,
  lib,
  stdenv,
  bash,
  runCommand,
  replaceVars,
  writeText,
  writeShellScript,
  dbus,
  pkgsStatic,
  vulkan-loader,
  mesa,
  sway,
  wayland,
  glfw,
  libxkbcommon,
  libdecor,
  foot,
  dejavu_fonts,
  python3,
  bat,
  symlinkJoin,
}:

let
  name = "hello-ouster-terminal";

  appRuntime = pkgs.callPackage ../../nix/app-runtime.nix { };

  ousterCommon = import ./ouster-app-common.nix {
    inherit lib writeShellScript;
    pickOusterSensorPy = ./pick_ouster_sensor.py;
    logTag = name;
    useCustomPorts = true;
  };

  inherit (ousterCommon)
    ousterUdpFirewallRange
    ousterLidarDataPort
    ousterImuDataPort
    ;

  ouster-sdk = ps.ousterSdk;

  # HACK (x86 Mesa): host libgallium NEEDEDs libLLVM via DT_RUNPATH; ld.so does not
  # fall back to LD_LIBRARY_PATH for that edge, so a missing/unbound llvm store root in
  # the app unit namespace breaks gallium. LD_PRELOAD forces our libLLVM into the
  # link map before gallium loads. Prefer fixing jetstream host GL extension roots
  # + BindReadOnlyPaths so host RUNPATH works, then drop this closure and preload.
  #
  # Symptom was ouster-cli viz only: Sway/wlroots/foot already use host EGL/GBM; viz
  # (GLFW Wayland client, second GL context) was where gallium/LLVM failed to resolve.
  # Preload is unit-wide in run-wayland.sh, not scoped to viz — viz was the canary.
  #
  # Copy shared libs only — a store ref to libllvm.lib pulls the whole llvm pkg (~500MiB).
  # Omitted on aarch64 (zedbox-mini-nx / Tegra): NVIDIA path, no DDI LLVM blob.
  mesaLlvmRuntimeClosure = lib.optionals stdenv.hostPlatform.isx86_64 [
    (runCommand "${name}-libllvm-preload" { } ''
      mkdir -p "$out/lib"
      src=${mesa.llvmPackages.libllvm.lib}/lib
      ver=$(basename "$(readlink -f "$src/libLLVM.so")")
      cp -L "$src/$ver" "$out/lib/"
      ln -sfn "$ver" "$out/lib/libLLVM.so"
    '')
  ];
  ldPreloadValues = lib.concatMap (p: [ "${p}/lib/libLLVM.so" ]) mesaLlvmRuntimeClosure;
  ldPreloadExport = lib.optionalString (ldPreloadValues != [ ]) ''
    if [[ "$_tegra_l4t" -eq 0 ]]; then
      export LD_PRELOAD="${lib.concatStringsSep ":" ldPreloadValues}''${LD_PRELOAD:+:''$LD_PRELOAD}"
    fi
  '';

  jp = ps.hostUnitDefaults {
    hostGpuChecksCfg = {
      enable = true;
      gbmBackends = true;
    };
  };

  # Busybox first so POSIX staples (ls, cp, grep, …) resolve before other bin/ (debug shells).
  serviceBinPath = lib.makeBinPath [
    pkgsStatic.busybox
    bash
    bat
    dbus
    foot
    sway
    ouster-sdk
    python3
  ];

  # In-kiosk README (bat in foot). See README.md: # headings (not
  # underline titles), inline `command` OK, no triple-backtick fences (indent examples),
  # no bold; cross-ref sway.md / viz.md for layout and viz details.
  kioskReadme = writeText "${name}-docs-README.md" ''
    # ${name}

    Sway + foot kiosk demo with lidar viz via ouster-cli.

    ## Network

    - UDP ${ousterUdpFirewallRange} leased from Jetstream for the kiosk lifetime.
    - Point the sensor at this host (ports +100 vs appliance defaults 7502–7504):
      - Lidar ${toString ousterLidarDataPort}/udp
      - IMU ${toString ousterImuDataPort}/udp

    ## Docs

    More in ~/docs (bat -p FILE), e.g.:

    - `bat -p sway.md`
    - `bat -p viz.md`

    Common shell utilities (ls, cat, grep, cp, …) are busybox on PATH.

    ## Debug shell

    From the host as root, attach to the service mount namespace and run bash.
    /bin in the DDI has bash and busybox applets (ls, …):

        systemd-run -tp JoinsNamespaceOf=${name}.service bash

    ## Sway key bindings

    - Super+Shift+Enter — new terminal
    - Super+F — fullscreen
  '';

  kioskSwayMd = writeText "${name}-docs-sway.md" ''
    # Sway (kiosk image)

    Minimal bundled config; not full default Sway binds.

    ## Layout

    - Left: foot shell (starts in ~/docs).
    - Right: tile for ouster-cli viz after sensor pick.

    ## Keys

    - Super+Shift+Enter — new foot window
    - Super+F — fullscreen focus
  '';

  kioskVizMd = writeText "${name}-docs-viz.md" ''
    # Ouster viz (ouster-cli)

    Ouster SDK Autorun foot: discover → pick sensor → ouster-cli source … viz (Wayland/GLFW).

    ## Sensor picker

    - 10s to enter a list number; else auto-pick first RUNNING in discover JSON.
    - Among those, auto-pick uses highest semver parsed from image/build rev (tie: list order).
    - If no RUNNING field in JSON, falls back to all discovered.
    - Ctrl-C at prompt re-runs discover.

    ## Ports

    - Lidar ${toString ousterLidarDataPort}/udp
    - IMU ${toString ousterImuDataPort}/udp

    Point sensor UDP at this host on those ports (see README.md).

    ## After exit

    Viz exits → short delay → picker again. Logs: journalctl, unit ${name}.
  '';

  appBashrc = writeText "${name}-bashrc" ''
    case $- in *i*) ;; *) return ;; esac
    PS1='\[\033[38;5;214m\]\u\[\033[38;5;242m\]@\[\033[38;5;80m\]kiosk\[\033[0m\] \[\033[38;5;153m\]\W\[\033[0m\] \[\033[38;5;114m\]>\[\033[0m\] '
    [[ -d "$HOME/docs" ]] && cd "$HOME/docs"
  '';

  batExe = lib.getExe bat;

  directionsLaunch = writeShellScript "${name}-directions" ''
    if [[ -r "$HOME/docs/README.md" ]]; then
      ${batExe} --no-paging -p "$HOME/docs/README.md"
    fi
    printf '\n\n'
    exec ${bash}/bin/bash -i
  '';

  fontsConf = ps.mkFontconfigConf {
    name = "${name}-fonts.conf";
    inherit dejavu_fonts;
    cacheDir = "/run/${name}/fontconfig-cache";
  };

  # Host EGL/GLVND/GBM first (/run/opengl-driver + generator); DDI carries GLFW/Wayland only.
  clientLd = lib.makeLibraryPath [
    vulkan-loader
    wayland
    glfw
    libxkbcommon
    libdecor
    stdenv.cc.cc.lib
  ];

  ousterVizLoop = ousterCommon.ousterVizLoopSh {
    caBundlePath = ps.caBundlePath;
    wayland = true;
    swayBin = sway;
    swayLayoutReady = true;
    vizExitFootHint = true;
  };

  swayAutostart = writeShellScript "${name}-sway-autostart" ''
    set +e
    layout_ready="$HOME/layout-ready"
    rm -f "$layout_ready"
    swaymsg() { command ${sway}/bin/swaymsg "$@" >/dev/null || true; }

    # Left column: general shell; viz loop foot spawns beside it.
    ${footExe} --config=${footIni} -T Terminal ${directionsLaunch} &
    sleep 0.45
    swaymsg split vertical

    (
      while true; do
        ${footExe} --config=${footIni} -T "Ouster SDK Autorun" ${ousterVizLoop} || true
        sleep 1
      done
    ) &
    sleep 0.55

    # Wrap the stacked feet in a horizontal split and focus the empty right pane for GLFW.
    swaymsg focus parent
    swaymsg focus parent
    swaymsg splith
    swaymsg focus right
    : > "$layout_ready"
  '';

  footExe = lib.getExe foot;
  # Shared by sway `exec` and Mod4+Shift+Return so every Foot uses the same font as viz.
  footIni = writeText "${name}-foot.ini" ''
    [main]
    font=DejaVu Sans Mono:size=14
  '';

  swayConfig = writeText "${name}-sway.conf" ''
    # Stock Sway/i3 style (this config does not include the full default keymap).
    bindsym Mod4+f fullscreen
    bindsym Mod4+Shift+Return exec ${footExe} --config=${footIni} ${bash}/bin/bash -i

    exec ${swayAutostart}
  '';

  # RootImage `/bin` for debug shell via systemd-run JoinsNamespaceOf (rootInteractiveBin).
  # Busybox applets are seeded by image-builder.nix; list full tools only here.
  rootInteractiveBin = symlinkJoin {
    name = "${name}-root-interactive-bin";
    paths = [
      bash
    ];
  };

  ddiBinPathExport = "export PATH=\"/bin:/usr/bin''${"PATH:+:$PATH"}\"";

  ddiProfileBinPath = writeText "${name}-profile-d-binpath.sh" ddiBinPathExport;
  ddiEtcBashBashrc = writeText "${name}-etc-bash.bashrc" ddiBinPathExport;
  ddiRootBashrc = writeText "${name}-root-bashrc" ''
    ${ddiBinPathExport}
    case $- in *i*)
      [[ -f /etc/bash.bashrc ]] && . /etc/bash.bashrc
    ;;
    esac
  '';
  ddiJsBashrc = writeText "${name}-js-bashrc" ddiBinPathExport;

  innerRun = replaceVars ./run-wayland.sh.in {
    bash = "${bash}";
    inherit clientLd ldPreloadExport;
    clientPath = serviceBinPath;
    fontsConf = "${fontsConf}";
    swayConfig = "${swayConfig}";
    bashrc = "${appBashrc}";
  };

  runLoopPack = ps.mkAppRunWrapper {
    inherit name innerRun bash;
    extraInstall = ''
      mkdir -p "$out/docs"
      install -m444 ${kioskReadme} "$out/docs/README.md"
      install -m444 ${kioskSwayMd} "$out/docs/sway.md"
      install -m444 ${kioskVizMd} "$out/docs/viz.md"
    '';
  };
  runLoop = "${runLoopPack}/run";

  appManifest = writeText "${name}-app-manifest.json" ''
    {
      "id": "${name}",
      "version": "${ps.imageVersion}",
      "display_name": "Hello Ouster Terminal",
      "nav": { "label": "Hello Ouster Terminal", "path": "/app/${name}" },
      "ui": { "mode": "external" },
      "backend": { "mode": "none" },
      "portable": { "displayService": true, "gpu": "wayland" }
    }
  '';

  rootExtensionRelease = writeText "${name}-extension-release" ''
    ID=${name}
    VERSION_ID=1
  '';
in
ps.makeAppImage {
  inherit name rootInteractiveBin;
  copyFiles =
    ps.mkPasswdGroupStubCopyFiles "hot"
    ++ ps.waylandAppBaseCopyFiles
    ++ [
      {
        path = "usr/lib/jetstream/mission-control/manifest.json";
        source = appManifest;
      }
      {
        path = "usr/lib/extension-release.d/extension-release.hello-ouster-terminal";
        source = rootExtensionRelease;
      }
      {
        path = "etc/profile.d/zz-${name}-binpath.sh";
        source = ddiProfileBinPath;
      }
      {
        path = "etc/bash.bashrc";
        source = ddiEtcBashBashrc;
      }
      {
        path = "root/.bashrc";
        source = ddiRootBashrc;
      }
      {
        path = "home/js/.bashrc";
        source = ddiJsBashrc;
      }
    ];
  unit = ps.mkGpuWaylandUnit {
    inherit name jp;
    description = "hello-ouster-terminal (Sway Wayland kiosk + foot; native GLFW Wayland viz; systemd-run /bin)";
    execStart = "${appRuntime}/bin/app-runtime-process ${name} udp ${ousterUdpFirewallRange} ${runLoop}";
    serviceType = "notify";
    runtimeDirectory = name;
    useTty = true;
    bindHostEtc = true;
    user = "js";
    extraUnit = ''
      After=mission-control.service
      Wants=mission-control.service
    '';
    extraService = ''
      SupplementaryGroups=tty seat
      TimeoutStartSec=90
      RuntimeDirectoryMode=0700
    '';
  };
  runtimeClosures = [
    runLoopPack
    appRuntime
    ouster-sdk
    rootInteractiveBin
  ]
  ++ mesaLlvmRuntimeClosure
  ++ jp.runtimeClosures;
}
