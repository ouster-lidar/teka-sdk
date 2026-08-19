# Portable DDI: Sway Wayland + SimpleViz (ps.ousterSdk source build, native GLFW Wayland) + MC UI.
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
  grim,
  wtype,
  dejavu_fonts,
  python312Packages,
}:

let
  pe = import ../../nix/portable-app.nix {
    inherit
      ps
      pkgs
      lib
      runCommand
      ;
  };

  name = "hello-ouster-world";

  pythonForApp = python312Packages.python;

  ouster-sdk = ps.ousterSdk;

  pythonEnv = python312Packages.python.withPackages (ps: [
    ouster-sdk
  ]);

  jpDisplay = ps.jetstreamPortableService {
    hostGpuChecksCfg = {
      gbmBackends = true;
    };
  };

  jpApi = pe.mcJetstreamPortable;

  howSrc = pe.mcRustSrc { srcDir = ./.; };

  howBin = pe.buildMcCrate2nix {
    cargoNix = ./Cargo.nix;
    crateName = "hello-ouster-world";
    filteredSrc = howSrc;
    nativeBuildInputs = pe.mcOpensslNative;
    meta = {
      mainProgram = "hello-ouster-world";
    };
  };

  helloOusterWorldPy = ./hello_ouster_world.py;
  ousterVizUtilsPy = ./viz_utils.py;

  vizCmd = writeShellScript "${name}-viz" ''
    set -euo pipefail
    export PYTHONPATH="/usr/lib/${name}:/usr/lib/${name}/dynamo-utils''${PYTHONPATH:+:$PYTHONPATH}"
    export SSL_CERT_FILE="''${SSL_CERT_FILE:-${ps.caBundlePath}}"
    export GLFW_PLATFORM=wayland
    export GLFW_WAYLAND_APP_ID=hello-ouster-world
    unset DISPLAY 2>/dev/null || true
    echo "hello-ouster-world: starting SimpleViz (GLFW Wayland)" >&2
    exec ${pythonEnv}/bin/python3 /usr/lib/${name}/hello_ouster_world.py "$@"
  '';

  # HACK (x86 Mesa): same gallium/LLVM preload as hello-ouster-terminal — SimpleViz is
  # the second GLFW Wayland GL context where host RUNPATH can fail in the portable unit.
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
        exec ${vizCmd}
  '';

  swayConfig = writeText "${name}-sway.conf" ''
    xwayland disable
    # Do not pin Ouster Viz to HEADLESS here — Display places it on HDMI or
    # HEADLESS based on the HDMI toggle. A for_window move would fight that.
    exec ${swayStartup}
  '';

  clientPath = lib.makeBinPath [
    pkgsStatic.busybox
    bash
    dbus
    sway
    grim
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

  runLoopPack = ps.mkPortableRunWrapper {
    inherit name innerRun bash;
  };
  runLoop = "${runLoopPack}/run";

  webTree = pe.mcWebTree {
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

  mcManifest = writeText "mission-control-manifest.json" ''
    {
      "id": "hello-ouster-world",
      "version": "1.0.0",
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
    Environment=HELLO_OUSTER_WORLD_GRIM=${lib.getExe grim}
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
    ConditionPathExists=/var/lib/${name}/display.enabled
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
    ${ps.jetstreamPortableHostEtcBindPaths}
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
ps.makePortableErofsImage {
  inherit name;
  copyFiles =
    ps.mkPasswdGroupStubCopyFiles "how"
    ++ ps.waylandPortableBaseCopyFiles
    ++ pe.mcManifestCopyFiles {
      inherit name mcManifest webTree;
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
    After=mission-control.service systemd-user-sessions.service network.target network-online.target time-sync.target seatd.service
    Wants=network-online.target time-sync.target seatd.service
    ${ps.jetstreamMcPortableBackendUnitLines}

    [Service]
    ${ps.mcPortableNotifyServiceLines}
    User=root
    ${jpApi.serviceFragment}
    ${commonEnv}
    Environment=RUST_LOG=info
    ExecStart=${lib.getExe howBin}
    SyslogIdentifier=${name}
    Restart=always
    RestartSec=3
    StandardOutput=journal
    StandardError=journal
    ${ps.jetstreamPortableHostEtcBindPaths}
    BindReadOnlyPaths=-/run/seatd.sock
    ${dynamoRuntimeBind}
    ${demoOsfBind}
    RuntimeDirectory=${name}
    RuntimeDirectoryMode=0700
    StateDirectory=${name}
    StateDirectoryMode=0750
    SupplementaryGroups=tty seat
    ${ps.jetstreamMcPortableBackendServiceLines}

    [Install]
    WantedBy=jetstream-apps.target
  '';
  runtimeClosures = [
    howBin
    runLoopPack
    vizCmd
    pythonEnv
    ouster-sdk
    grim
    wtype
    sway
  ]
  ++ mesaLlvmRuntimeClosure
  ++ jpDisplay.runtimeClosures;
}
