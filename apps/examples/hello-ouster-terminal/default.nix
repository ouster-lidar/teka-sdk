{
  image =
    # App image DDI: Sway Wayland kiosk + foot, running ouster-cli viz against nixpkgs
    # GLFW / python3Packages.glfw on native Wayland (GL prefix resolver, MESA shader
    # cache, /bin bash+busybox, systemd-run README).
    {
      ps,
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
      replaceVarsWith,
      runtimeShell,
    }:

    let
      name = "hello-ouster-terminal";

      inherit (ps) tekaRunner;

      ousterUdpFirewallRange = "7000-8000";
      ousterLidarDataPort = 7602;
      ousterImuDataPort = 7603;

      ouster-sdk = ps.ousterSdk;

      # HACK (x86 Mesa): host libgallium NEEDEDs libLLVM via DT_RUNPATH; ld.so does not
      # fall back to LD_LIBRARY_PATH for that edge, so a missing/unbound llvm store root in
      # the app unit namespace breaks gallium. LD_PRELOAD forces our libLLVM into the
      # link map before gallium loads. Prefer fixing teka host GL extension roots
      # + BindReadOnlyPaths so host RUNPATH works, then drop this closure and preload.
      #
      # Symptom was ouster-cli viz only: Sway/wlroots/foot already use host EGL/GBM; viz
      # (GLFW Wayland client, second GL context) was where gallium/LLVM failed to resolve.
      # Preload is unit-wide in run-wayland.sh, not scoped to viz — viz was the canary.
      #
      # Copy shared libs only — a store ref to libllvm.lib pulls the whole llvm pkg (~500MiB).
      # Omitted on aarch64 (zedbox-mini / Tegra): NVIDIA path, no DDI LLVM blob.
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
      kioskReadme = replaceVars ./docs/kiosk.md.in {
        inherit ousterUdpFirewallRange;
        ousterLidarDataPort = toString ousterLidarDataPort;
        ousterImuDataPort = toString ousterImuDataPort;
      };

      kioskVizMd = replaceVars ./docs/viz.md.in {
        ousterLidarDataPort = toString ousterLidarDataPort;
        ousterImuDataPort = toString ousterImuDataPort;
      };

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

      ousterVizLoop = replaceVarsWith {
        name = "${name}-ouster-viz-loop";
        src = ./scripts/viz-loop.sh.in;
        isExecutable = true;
        replacements = {
          inherit runtimeShell;
          inherit (ps) caBundlePath;
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
          inherit runtimeShell;
          inherit sway;
          inherit footExe;
          footIni = "${footIni}";
          inherit directionsLaunch;
          inherit ousterVizLoop;
        };
      };

      footExe = lib.getExe foot;
      # Shared by sway `exec` and Mod4+Shift+Return so every Foot uses the same font as viz.
      footIni = ./config/foot.ini;

      swayConfig = replaceVars ./config/sway.conf.in {
        inherit footExe;
        footIni = "${footIni}";
        inherit bash;
        inherit swayAutostart;
      };

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
      ddiRootBashrc = replaceVars ./config/root-bashrc.sh.in {
        inherit ddiBinPathExport;
      };
      ddiJsBashrc = writeText "${name}-js-bashrc" ddiBinPathExport;

      innerRun = replaceVars ./run-wayland.sh.in {
        bash = "${bash}";
        inherit clientLd ldPreloadExport;
        clientPath = serviceBinPath;
        fontsConf = "${fontsConf}";
        swayConfig = "${swayConfig}";
        bashrc = "${./config/bashrc}";
      };

      runLoopPack = ps.mkAppRunWrapper {
        inherit name innerRun bash;
        extraInstall = ''
          mkdir -p "$out/docs"
          install -m444 ${kioskReadme} "$out/docs/README.md"
          install -m444 ${./docs/sway.md} "$out/docs/sway.md"
          install -m444 ${kioskVizMd} "$out/docs/viz.md"
        '';
      };
      runLoop = "${runLoopPack}/run";

      appManifest = replaceVars ./manifest.json {
        imageVersion = ps.imageVersion;
      };
    in
    ps.makeAppImage {
      inherit name rootInteractiveBin;
      copyFiles =
        ps.mkPasswdGroupStubCopyFiles "hot"
        ++ ps.waylandAppBaseCopyFiles
        ++ [
          {
            path = "usr/lib/teka/mission-control/manifest.json";
            source = appManifest;
          }
          {
            path = "usr/lib/extension-release.d/extension-release.hello-ouster-terminal";
            source = ./config/extension-release;
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
        execStart = "${tekaRunner}/bin/teka-runner ${name} udp ${ousterUdpFirewallRange} ${runLoop}";
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
        tekaRunner
        ouster-sdk
        rootInteractiveBin
      ]
      ++ mesaLlvmRuntimeClosure
      ++ jp.runtimeClosures;
    };
}
