{ pkgs, teka }:
let
  name = teka.appId;
  ousterLogo =
    pkgs.runCommand "${name}-ouster-logo.png"
      {
        nativeBuildInputs = [ pkgs.buildPackages.librsvg ];
      }
      ''
        export FONTCONFIG_FILE=${teka.lib.wayland.fontsConf}
        rsvg-convert -w 880 -h 220 ${./ouster-logo.svg} -o "$out"
      '';

  # Sway's hide_cursor still shows the pointer on every motion (USB mouse or
  # browser input), so use a 1x1 transparent Xcursor for the compositor and
  # GLFW instead.
  hiddenCursorIcons = pkgs.runCommand "${name}-hidden-cursor" { } ''
    d=$out/share/icons/hidden/cursors
    mkdir -p $d
    printf 'Xcur\x10\x00\x00\x00\x00\x00\x01\x00\x01\x00\x00\x00\x02\x00\xfd\xff\x18\x00\x00\x00\x1c\x00\x00\x00\x24\x00\x00\x00\x02\x00\xfd\xff\x18\x00\x00\x00\x01\x00\x00\x00\x01\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00' > $d/default
    ln -s default $d/left_ptr
  '';

  statusBarCmd = pkgs.writeShellScript "${name}-status-bar" ''
    exec ${
      pkgs.python3Packages.python.withPackages (ps: [ ps.ouster-sdk ])
    }/bin/python3 ${./status_bar.py}
  '';
in
pkgs.callPackage ./viewer.nix {
  inherit teka;
  src = ./.;
  envPrefix = "HELLO_OUSTER_WORLD";
  extraBuildInputs = [ pkgs.gst_all_1.gst-plugins-good ];
  extraEnvironment.TEKA_SENSORS_APP_ID = "sensors";
  swayReplacements = { inherit ousterLogo statusBarCmd; };
  cursorPath = "${hiddenCursorIcons}/share/icons";
}
