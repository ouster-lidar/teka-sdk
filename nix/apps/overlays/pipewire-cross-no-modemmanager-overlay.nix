# Cross: nixpkgs PipeWire may pull ModemManager → libqmi, whose docs step breaks
# Meson cross ("pkg-config for build machine not found"). Portable cross images
# that ship PipeWire for capture/portal use do not need MM here.
_: prev: {
  pipewire = prev.pipewire.overrideAttrs (old: {
    buildInputs = prev.lib.filter (p: (prev.lib.getName p) != "modemmanager") old.buildInputs;
    mesonFlags =
      let
        flags = old.mesonFlags or [ ];
      in
      (builtins.filter (
        f: !(builtins.isString f && prev.lib.strings.hasInfix "bluez5-backend-native-mm" f)
      ) flags)
      ++ [ (prev.lib.mesonEnable "bluez5-backend-native-mm" false) ];
  });
}
