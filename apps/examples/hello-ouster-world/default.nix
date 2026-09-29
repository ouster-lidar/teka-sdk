# App image DDI: Sway Wayland + SimpleViz (ps.ousterSdk source build, native GLFW Wayland) + MC UI.
# OSF lives at /srv/recordings (host bind), not in this image or StateDirectory.
{
  ps,
  pkgs,
  lib,
  stdenv,
  bash,
  replaceVars,
  writeText,
  writeShellScript,
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
}:

let
  app = ps.appCrate;

  name = "hello-ouster-world";

  pythonForApp = python312Packages.python;

  ouster-sdk = ps.ousterSdk;

  pythonEnv = python312Packages.python.withPackages (ps: [
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

  howSrc = app.appRustSrc { srcDir = ./.; };

  gstNative = with pkgs; [
    pkg-config
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    wayland
    glib
  ];

  # DDI GStreamer. nvvidconv/nvjpegenc are L4T; NVIDIA userspace comes from
  # X-Jetstream-PortableGPU (not BindReadOnlyPaths of host /usr).
  gstPluginSystemPath = lib.concatStringsSep ":" [
    "${pkgs.gst_all_1.gstreamer}/lib/gstreamer-1.0"
    "${pkgs.gst_all_1.gst-plugins-base}/lib/gstreamer-1.0"
    "${pkgs.gst_all_1.gst-plugins-good}/lib/gstreamer-1.0"
    "${pkgs.gst_all_1.gst-plugins-bad}/lib/gstreamer-1.0"
  ];

  howBin =
    let
      crateOverridesForPkgs =
        targetPkgs:
        targetPkgs.defaultCrateOverrides
        // (import ./crate-config.nix { pkgs = targetPkgs; })
        // {
          app-runtime = app.strictRustWarnings;
          hello-ouster-world =
            attrs:
            app.strictRustWarnings attrs
            // {
              src = howSrc;
              preBuild = (attrs.preBuild or "") + ''
                export CARGO_CRATE_NAME="${attrs.libName or "hello_ouster_world"}"
              '';
              nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ targetPkgs.pkg-config ];
              buildInputs =
                (attrs.buildInputs or [ ])
                ++ (with targetPkgs; [
                  gst_all_1.gstreamer
                  gst_all_1.gst-plugins-base
                  glib
                  wayland
                ]);
            };
        };
      cross = import ../../nix/cross-crate2nix.nix {
        inherit lib pkgs crateOverridesForPkgs;
      };
    in
    (cross {
      cargoNix = ./Cargo.nix;
      packageId = "hello-ouster-world";
    }).overrideAttrs
      (old: {
        nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ app.appOpensslNative ++ gstNative;
        meta = (old.meta or { }) // {
          mainProgram = "hello-ouster-world";
        };
      });

  howApiCmd = writeShellScript "${name}-api" ''
    set -euo pipefail
    # Nix GST (appsrc/appsink). Keep generator GST_PLUGIN_PATH
    # (l4t-gstreamer: nvvidconv / nvjpegenc). Do not unset it.
    export GST_PLUGIN_SYSTEM_PATH="${gstPluginSystemPath}"
    unset GST_PLUGIN_SYSTEM_PATH_1_0 2>/dev/null || true
    # Generator LD_LIBRARY_PATH lists Mesa libgbm before nvidia-l4t-gbm; Mesa
    # then looks for dri_gbm.so. Prefer L4T GBM backends.
    IFS=':' read -r -a _ldpath <<< "''${LD_LIBRARY_PATH:-}"
    for _d in "''${_ldpath[@]}"; do
      if [[ "$_d" == *nvidia-l4t-gbm* ]]; then
        export LD_LIBRARY_PATH="$_d''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
        if [[ -d "$_d/gbm" ]]; then
          export GBM_BACKENDS_PATH="$_d/gbm"
        fi
        break
      fi
    done
    # L4T nvvidconv/nvjpegenc may shell out to `lsmod | grep`. PortableGPU PATH
    # has neither busybox applets nor kmod; keep /bin as fallback.
    export PATH="${
      lib.makeBinPath [
        pkgsStatic.busybox
        pkgs.kmod
      ]
    }:/bin:/usr/bin''${PATH:+:$PATH}"
    exec ${lib.getExe howBin} "$@"
  '';

  helloOusterWorldPy = ./hello_ouster_world.py;
  ousterVizUtilsPy = ./viz_utils.py;

  vizCmd = writeShellScript "${name}-viz" ''
    set -euo pipefail
    export PYTHONPATH="/usr/lib/${name}:/usr/lib/${name}/dynamo-utils''${PYTHONPATH:+:$PYTHONPATH}"
    export SSL_CERT_FILE="''${SSL_CERT_FILE:-${ps.caBundlePath}}"
    export GLFW_PLATFORM=wayland
    export GLFW_WAYLAND_APP_ID=hello-ouster-world
    export __GL_SYNC_TO_VBLANK=1
    unset DISPLAY 2>/dev/null || true
    echo "hello-ouster-world: starting SimpleViz (GLFW Wayland)" >&2
    ${pythonEnv}/bin/python3 /usr/lib/${name}/hello_ouster_world.py "$@"
  '';

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

  swayStartup = writeShellScript "${name}-sway-startup" ''
        set -euo pipefail

        # Ensure a persistent HEADLESS output (browser/home canvas).
        for _ in $(seq 1 50); do
          outputs="$(swaymsg -t get_outputs 2>/dev/null || true)"
          if [[ "$outputs" == *HEADLESS* ]]; then
            break
          fi
          swaymsg create_output >/dev/null 2>&1 || true
          sleep 0.1
        done

        headless_name="$(
          swaymsg -t get_outputs 2>/dev/null \
            | sed -n 's/.*"name": "\(HEADLESS[^"]*\)".*/\1/p' \
            | head -n1
        )"
        if [[ -n "$headless_name" ]]; then
          swaymsg output "$headless_name" enable >/dev/null 2>&1 || true
          swaymsg output "$headless_name" resolution 1920x1080 >/dev/null 2>&1 || true
          echo "hello-ouster-world: headless home output=$headless_name" >&2
        else
          echo "hello-ouster-world: WARNING no HEADLESS output after create_output" >&2
        fi

        for _ in $(seq 1 100); do
          outputs="$(swaymsg -t get_outputs 2>/dev/null || true)"
          if [[ "$outputs" == *HEADLESS* && "$outputs" == *'"active": true'* ]]; then
            break
          fi
          sleep 0.2
        done

        phys_name="$(
          swaymsg -t get_outputs 2>/dev/null | ${pythonEnv}/bin/python3 -c '
    import json, sys
    try:
        outs = json.load(sys.stdin)
    except Exception:
        sys.exit(0)
    for o in outs:
        n = o.get("name") or ""
        if o.get("active") and not str(n).startswith("HEADLESS"):
            print(n)
            break
    ' || true
        )"
        if [[ -n "$phys_name" ]]; then
          swaymsg focus output "$phys_name" >/dev/null 2>&1 || true
          echo "hello-ouster-world: focus physical output=$phys_name for SimpleViz" >&2
        else
          swaymsg workspace 1 >/dev/null 2>&1 || true
        fi
        sleep 0.5

        echo "hello-ouster-world: launching SimpleViz (GLFW Wayland)" >&2
        while true; do
          ${vizCmd} || true
          echo "hello-ouster-world: SimpleViz exited $?; restarting in 2s" >&2
          sleep 2
        done
  '';

  swayConfig = writeText "${name}-sway.conf" ''
    # Kiosk: one client, no titlebars/split chrome.
    font pango:monospace 1
    default_border none
    default_floating_border none
    hide_edge_borders both
    output * bg #000000 solid_color
    # Do not pin Ouster Viz to HEADLESS here — Display places it on HDMI or
    # HEADLESS based on the HDMI toggle. A for_window move would fight that.
    for_window [app_id="hello-ouster-world"] border none, fullscreen enable
    for_window [title="Ouster Viz"] border none, fullscreen enable
    exec ${swayStartup}
  '';

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

  appManifest = writeText "app-manifest.json" ''
    {
      "id": "hello-ouster-world",
      "version": "${ps.imageVersion}",
      "display_name": "hello-ouster-world",
      "nav": {
        "label": "hello-ouster-world",
        "path": "/app/hello-ouster-world",
        "section": "apps",
        "order": 20,
        "panels": [
          { "label": "Display", "panel": "display" }
        ]
      },
      "ui": {
        "mode": "static",
        "title": "hello-ouster-world",
        "template": "display.html",
        "styles": ["display.css", "xterm.css"]
      },
      "backend": {
        "mode": "proxy",
        "runtime_listen": true,
        "health_path": "/api/v1/status",
        "card_stream_path": "/api/v1/cards/stream"
      },
      "registers": [
        { "alias": "hello-ouster-world", "panel": "display" }
      ],
      "app_cards": [
        {
          "card_id": "hello-ouster-world",
          "title": "Hello Ouster World",
          "description": "Example visualizer app with Ouster Dynamo integration",
          "icon": "📡",
          "link_alias": "hello-ouster-world"
        }
      ],
      "auth": {
        "scheme": "ojs-proxy-identity",
        "version": "0.1.0"
      },
      "ports": [
        { "port": 7502, "proto": "udp" },
        { "port": 7503, "proto": "udp" }
      ],
      "portable": {
        "displayService": true,
        "gpu": "wayland"
      }
    }
  '';

  commonEnv = ''
    Environment=HELLO_OUSTER_WORLD_VIZ=${runLoop}
    Environment=HELLO_OUSTER_WORLD_SHELL=${bash}/bin/bash
    Environment=HELLO_OUSTER_WORLD_CWD=/var/lib/${name}
    Environment=HELLO_OUSTER_WORLD_PATH=${clientPath}
    Environment=HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR=/run/${name}
    Environment=HELLO_OUSTER_WORLD_WAYLAND_DISPLAY=wayland-1
    Environment=HELLO_OUSTER_WORLD_SWAYMSG=${sway}/bin/swaymsg
    Environment=HELLO_OUSTER_WORLD_WTYPE=${lib.getExe wtype}
    Environment=HELLO_OUSTER_WORLD_CONSOLE_LOG=/run/${name}/console.log
    Environment=STATE_DIRECTORY=/var/lib/${name}
    Environment=APP_ID=${name}
    Environment=SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt
  '';

  dynamoRuntimeBind = "BindPaths=-/run/ouster-dynamo";

  # Host /srv exists; /srv/recordings does not until first mkdir. Bind the parent
  # (no '-' skip) so create_dir_all in the API is not EROFS.
  demoOsfBind = "BindPaths=/srv";

  libPyFiles = [
    {
      path = "usr/lib/${name}/hello_ouster_world.py";
      source = helloOusterWorldPy;
    }
    {
      path = "usr/lib/${name}/viz_utils.py";
      source = ousterVizUtilsPy;
    }
    {
      path = "usr/lib/${name}/source_config.py";
      source = ./source_config.py;
    }
    {
      path = "usr/lib/${name}/osf_socket.py";
      source = ../../nix/osf-socket/osf_socket.py;
    }
    {
      path = "usr/lib/${name}/dynamo-utils/detect_utils.py";
      source = ./dynamo-utils/detect_utils.py;
    }
    {
      path = "usr/lib/${name}/dynamo-utils/dynamo_viz_utils.py";
      source = ./dynamo-utils/dynamo_viz_utils.py;
    }
    {
      path = "usr/lib/${name}/dynamo-utils/ouster_dynamo_api.py";
      source = ./dynamo-utils/ouster_dynamo_api.py;
    }
    {
      path = "usr/lib/${name}/dynamo-utils/configs/__init__.py";
      source = ./dynamo-utils/configs/__init__.py;
    }
    {
      path = "usr/lib/${name}/dynamo-utils/configs/viz_config.py";
      source = ./dynamo-utils/configs/viz_config.py;
    }
    {
      path = "usr/lib/${name}/dynamo-utils/configs/viz_configs.json";
      source = ./dynamo-utils/configs/viz_configs.json;
    }
    {
      path = "usr/lib/${name}/dynamo-utils/configs/detect_configs.json";
      source = ./dynamo-utils/configs/detect_configs.json;
    }
  ];

  displayUnit = writeText "${name}-display.service" ''
    [Unit]
    Description=hello-ouster-world display (Sway + SimpleViz)
    Documentation=file:///usr/lib/jetstream/mission-control/manifest.json
    X-Jetstream-PortableGPU=wayland
    After=${name}.service seatd.service
    Requires=${name}.service
    Wants=seatd.service
    Conflicts=getty@tty1.service display-manager.service

    [Service]
    Type=simple
    User=root
    ${jpDisplay.serviceFragment}
    ${commonEnv}
    ExecStart=${runLoop}
    SyslogIdentifier=${name}-display
    Restart=always
    RestartSec=3
    StandardOutput=journal
    StandardError=journal
    ${ps.hostEtcBindPaths}
    BindReadOnlyPaths=-/run/seatd.sock
    ${dynamoRuntimeBind}
    ${demoOsfBind}
    RuntimeDirectory=${name}
    RuntimeDirectoryMode=0700
    StateDirectory=${name}
    StateDirectoryMode=0750
    SupplementaryGroups=tty seat
    KillMode=mixed
    TimeoutStopSec=15

    [Install]
    WantedBy=jetstream-apps.target
  '';
in
ps.makeAppImage {
  inherit name;
  copyFiles =
    ps.mkPasswdGroupStubCopyFiles "how"
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
  unit = writeText "${name}.service" ''
    [Unit]
    Description=hello-ouster-world (MC Console/Display API)
    Documentation=file:///usr/lib/jetstream/mission-control/manifest.json
    # Screencopy nvvidconv + nvjpegenc. cuda (not wayland) so the generator does not
    # Conflicts= the display compositor. Do not BindReadOnlyPaths=/run/opengl-driver.
    X-Jetstream-PortableGPU=cuda
    After=mission-control.service systemd-user-sessions.service network.target network-online.target time-sync.target seatd.service
    Wants=network-online.target time-sync.target seatd.service
    ${ps.appBackendUnitLines}

    [Service]
    ${ps.appNotifyServiceLines}
    User=root
    ${jpApi.serviceFragment}
    ${commonEnv}
    Environment=RUST_LOG=info
    ExecStart=${howApiCmd}
    SyslogIdentifier=${name}
    Restart=always
    RestartSec=3
    StandardOutput=journal
    StandardError=journal
    PrivateDevices=no
    SupplementaryGroups=tty seat video render
    ${ps.hostEtcBindPaths}
    BindReadOnlyPaths=-/run/seatd.sock
    ${dynamoRuntimeBind}
    ${demoOsfBind}
    RuntimeDirectory=${name}
    RuntimeDirectoryMode=0700
    StateDirectory=${name}
    StateDirectoryMode=0750
    ${ps.appBackendServiceLines}

    [Install]
    WantedBy=jetstream-apps.target
  '';
  runtimeClosures = [
    howBin
    howApiCmd
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
}
