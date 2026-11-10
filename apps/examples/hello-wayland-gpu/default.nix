{
  image =
    {
      ps,
      lib,
      stdenv,
      bash,
      replaceVars,
      writeText,
      dbus,
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
      swayStartup = replaceVarsWith {
        name = "hello-wayland-gpu-sway-startup";
        src = ./scripts/sway-startup.sh.in;
        isExecutable = true;
        replacements = {
          inherit runtimeShell;
          inherit vkcubeLoop;
          inherit glmark2Loop;
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
    };
}
