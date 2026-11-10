{
  image =
    # App image DDI: Sway Wayland + SimpleViz (ps.ousterSdk source build, native GLFW Wayland) + MC UI.
    # OSF lives at /srv/recordings (host bind), not in this image or StateDirectory.
    {
      ps,
      pkgs,
      lib,
      stdenv,
      bash,
      replaceVars,
      runCommand,
      dbus,
      pkgsStatic,
      vulkan-loader,
      mesa,
      sway,
      wayland,
      glfw,
      libxkbcommon,
      libdecor,
      wtype,
      dejavu_fonts,
      python312Packages,
      replaceVarsWith,
      runtimeShell,
    }:

    let
      app = ps.appCrate;

      name = "hello-ouster-dynamo";

      ouster-sdk = ps.ousterSdk;

      pythonEnv = python312Packages.python.withPackages (_: [
        ouster-sdk
      ]);

      jpDisplay = ps.hostUnitDefaults {
        hostGpuChecksCfg = {
          enable = true;
          gbmBackends = true;
        };
      };

      # CUDA, not wayland: two wayland GPU units Conflicts= each other and kill
      # the display compositor. hello-llm uses the same split.
      jpApi = app.appHostUnitDefaults;

      dynamoSrc = app.appRustSrc { srcDir = ./.; };

      # DDI GStreamer. nvvidconv/nvjpegenc are L4T; NVIDIA userspace comes from
      # X-Teka-PortableGPU (not BindReadOnlyPaths of host /usr).
      gstPluginSystemPath = lib.concatStringsSep ":" [
        "${pkgs.gst_all_1.gstreamer}/lib/gstreamer-1.0"
        "${pkgs.gst_all_1.gst-plugins-base}/lib/gstreamer-1.0"
        "${pkgs.gst_all_1.gst-plugins-good}/lib/gstreamer-1.0"
        "${pkgs.gst_all_1.gst-plugins-bad}/lib/gstreamer-1.0"
      ];

      dynamoBin = app.buildAppCrate {
        crateName = "hello-ouster-dynamo";
        filteredSrc = dynamoSrc;
        nativeBuildInputs = [ pkgs.buildPackages.pkg-config ];
        buildInputs = with pkgs; [
          gst_all_1.gstreamer
          gst_all_1.gst-plugins-base
          glib
          wayland
        ];
        meta.mainProgram = "hello-ouster-dynamo";
      };

      dynamoApiCmd = replaceVarsWith {
        name = "${name}-api";
        src = ./scripts/api.sh.in;
        isExecutable = true;
        replacements = {
          inherit runtimeShell;
          inherit gstPluginSystemPath;
          binPath = lib.makeBinPath [
            pkgsStatic.busybox
            pkgs.kmod
          ];
          program = lib.getExe dynamoBin;
        };
      };

      vizCmd = replaceVarsWith {
        name = "${name}-viz";
        src = ./scripts/viz.sh.in;
        isExecutable = true;
        replacements = {
          inherit runtimeShell;
          caBundlePath = ps.caBundlePath;
          inherit pythonEnv;
        };
      };

      # HACK (x86 Mesa): same gallium/LLVM preload as hello-ouster-terminal — SimpleViz is
      # the second GLFW Wayland GL context where host RUNPATH can fail in the app unit.
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

      fontsConf = ps.mkFontconfigConf {
        name = "${name}-fonts.conf";
        inherit dejavu_fonts;
        cacheDir = "/run/${name}/fontconfig-cache";
      };

      clientLd = lib.makeLibraryPath [
        vulkan-loader
        wayland
        glfw
        libxkbcommon
        libdecor
        stdenv.cc.cc.lib
      ];

      swayStartup = replaceVarsWith {
        name = "${name}-sway-startup";
        src = ./scripts/sway-startup.sh.in;
        isExecutable = true;
        replacements = {
          inherit runtimeShell;
          inherit pythonEnv;
          inherit vizCmd;
        };
      };

      swayConfig = replaceVars ./config/sway.conf.in {
        inherit swayStartup;
      };

      clientPath = lib.makeBinPath [
        pkgsStatic.busybox
        bash
        dbus
        sway
        wtype
        pythonEnv
        ouster-sdk
      ];

      innerRun = replaceVars ./run-wayland.sh.in {
        bash = "${bash}";
        inherit clientLd ldPreloadExport;
        inherit clientPath;
        fontsConf = "${fontsConf}";
        inherit swayConfig;
      };

      runLoopPack = ps.mkAppRunWrapper {
        inherit name innerRun bash;
      };
      runLoop = "${runLoopPack}/run";

      webTree = app.appWebTree {
        inherit name;
        webSrc = ./web;
        webFiles = [
          "index.html"
          "display.html"
          "display.js"
          "display.css"
        ];
        extraInstall = ''
          install -m644 ${./web/vendor/xterm.js} "$out/web/xterm.js"
          install -m644 ${./web/vendor/xterm.css} "$out/web/xterm.css"
          install -m644 ${./web/vendor/xterm-addon-fit.js} "$out/web/xterm-addon-fit.js"
        '';
      };

      appManifest = replaceVars ./manifest.json {
        imageVersion = ps.imageVersion;
      };

      libPyFiles =
        lib.mapAttrsToList
          (file: source: {
            path = "usr/lib/${name}/${file}";
            inherit source;
          })
          {
            "hello_ouster_dynamo.py" = ./hello_ouster_dynamo.py;
            "viz_utils.py" = ./viz_utils.py;
            "source_config.py" = ./source_config.py;
            "osf_socket.py" = ./osf-socket/osf_socket.py;
            "dynamo-utils/detect_utils.py" = ./dynamo-utils/detect_utils.py;
            "dynamo-utils/dynamo_viz_utils.py" = ./dynamo-utils/dynamo_viz_utils.py;
            "dynamo-utils/ouster_dynamo_api.py" = ./dynamo-utils/ouster_dynamo_api.py;
            "dynamo-utils/configs/__init__.py" = ./dynamo-utils/configs/__init__.py;
            "dynamo-utils/configs/viz_config.py" = ./dynamo-utils/configs/viz_config.py;
            "dynamo-utils/configs/viz_configs.json" = ./dynamo-utils/configs/viz_configs.json;
            "dynamo-utils/configs/detect_configs.json" = ./dynamo-utils/configs/detect_configs.json;
          };

      displayUnit = replaceVars ./hello-ouster-dynamo-display.service {
        serviceFragment = jpDisplay.serviceFragment;
        inherit runLoop;
        inherit bash;
        inherit clientPath;
        inherit sway;
        wtype = lib.getExe wtype;
        hostEtcBindPaths = ps.hostEtcBindPaths;
      };
    in
    ps.makeAppImage {
      inherit name;
      cargoChecks = dynamoBin.passthru.cargoChecks;
      copyFiles =
        ps.mkPasswdGroupStubCopyFiles "hod"
        ++ ps.waylandAppBaseCopyFiles
        ++ app.appManifestCopyFiles {
          inherit name appManifest webTree;
        }
        ++ libPyFiles
        ++ [
          {
            path = "srv/recordings/.keep";
            source = builtins.toFile "${name}-demo-osf-stub" "";
          }
          {
            path = "usr/lib/systemd/system/${name}-display.service";
            source = displayUnit;
          }
        ];
      unit = replaceVars ./hello-ouster-dynamo.service {
        appBackendUnitLines = ps.appBackendUnitLines;
        appNotifyServiceLines = ps.appNotifyServiceLines;
        serviceFragment = jpApi.serviceFragment;
        inherit runLoop;
        inherit bash;
        inherit clientPath;
        inherit sway;
        wtype = lib.getExe wtype;
        inherit dynamoApiCmd;
        hostEtcBindPaths = ps.hostEtcBindPaths;
        appBackendServiceLines = ps.appBackendServiceLines;
      };
      runtimeClosures = [
        dynamoBin
        dynamoApiCmd
        runLoopPack
        vizCmd
        pythonEnv
        ouster-sdk
        wtype
        sway
        pkgs.gst_all_1.gstreamer
        pkgs.gst_all_1.gst-plugins-base
        pkgs.gst_all_1.gst-plugins-good
        pkgs.gst_all_1.gst-plugins-bad
        pkgsStatic.busybox
        pkgs.kmod
      ]
      ++ mesaLlvmRuntimeClosure
      ++ jpDisplay.runtimeClosures;
    };
}
