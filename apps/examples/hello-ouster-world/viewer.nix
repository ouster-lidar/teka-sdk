{
  teka,
  cacert,
  src,
  envPrefix,
  extraPythonFiles ? [ ],
  extraPythonPath ? [ ],
  extraBuildInputs ? [ ],
  extraEnvironment ? { },
  extraBindPaths ? [ ],
  swayReplacements ? { },
  cursorPath ? null,
  pkgs,
  lib,
  stdenv,
  bash,
  replaceVars,
  pkgsStatic,
  vulkan-loader,
  sway,
  wayland,
  glfw,
  libxkbcommon,
  libdecor,
  wtype,
  python3Packages,
  replaceVarsWith,
  runtimeShell,
}:
let
  name = teka.appId;
  display = teka.lib.wayland;
  pythonSrc = lib.fileset.toSource {
    root = src;
    fileset = lib.fileset.unions (
      map (file: src + "/${file}") [
        "viewer.py"
        "viz_utils.py"
        "source_config.py"
      ]
      ++ extraPythonFiles
    );
  };
  pythonPath = "${pythonSrc}" + lib.concatMapStrings (dir: ":${pythonSrc}/${dir}") extraPythonPath;

  ouster-sdk = python3Packages.ouster-sdk;

  pythonEnv = python3Packages.python.withPackages (_: [
    ouster-sdk
  ]);

  # The NVIDIA userspace provides nvvidconv and nvjpegenc through the GPU profile.
  gstPluginSystemPath = lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" (
    with pkgs.gst_all_1;
    [
      gstreamer
      gst-plugins-base
      gst-plugins-good
      gst-plugins-bad
    ]
  );

  program = teka.lib.buildRustApp {
    srcDir = src;
    nativeBuildInputs = [ pkgs.buildPackages.pkg-config ];
    buildInputs = [
      pkgs.gst_all_1.gstreamer
      pkgs.gst_all_1.gst-plugins-base
    ]
    ++ extraBuildInputs
    ++ [
      pkgs.glib
      pkgs.wayland
    ];
  };

  apiCmd = replaceVarsWith {
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
      program = lib.getExe program;
    };
  };

  vizCmd = replaceVarsWith {
    name = "${name}-viz";
    src = ./scripts/viz.sh.in;
    isExecutable = true;
    replacements = {
      inherit runtimeShell;
      caBundlePath = "${cacert}/etc/ssl/certs/ca-bundle.crt";
      pythonEntrypoint = "${pythonSrc}/viewer.py";
      vizAppId = program.pname;
      inherit
        name
        pythonEnv
        pythonPath
        ;
    };
  };

  inherit (display) fontsConf;

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
      inherit name vizCmd;
    };
  };

  swayConfig = replaceVars (src + "/config/sway.conf.in") (
    { inherit swayStartup; } // swayReplacements
  );

  clientPath = lib.makeBinPath [
    pkgsStatic.busybox
    bash
    display.dbus
    sway
    wtype
    pythonEnv
    ouster-sdk
  ];

  runLoop = replaceVarsWith {
    src = ./run-wayland.sh.in;
    isExecutable = true;
    replacements = {
      bash = "${bash}";
      inherit clientLd;
      ldPreloadExport = display.mesaLlvmPreload;
      inherit
        clientPath
        envPrefix
        fontsConf
        name
        swayConfig
        ;
      cursorSetup = lib.optionalString (cursorPath != null) "export XCURSOR_PATH=${cursorPath}\n";
    };
  };

  launchEnvironment = {
    "${envPrefix}_VIZ" = "${runLoop}";
    "${envPrefix}_PATH" = clientPath;
    "${envPrefix}_SWAYMSG" = "${sway}/bin/swaymsg";
    "${envPrefix}_WTYPE" = lib.getExe wtype;
  };
in
teka.lib.mkAppImage {
  imports = [
    (import ./configuration.nix {
      inherit
        name
        envPrefix
        extraBindPaths
        extraEnvironment
        ;
    })
  ];

  services =
    lib.mapAttrs
      (_: execStart: {
        systemd.environment = launchEnvironment;
        systemd.serviceConfig.ExecStart = execStart;
      })
      {
        display = runLoop;
        main = apiCmd;
      };

  image = {
    packages = [
      (pkgs.writeTextDir "srv/recordings/.keep" "")
      (pkgs.linkFarm "${name}-web" { "${teka.appDir}/web" = src + "/web"; })
      teka.lib.webApi
    ];
  };
}
