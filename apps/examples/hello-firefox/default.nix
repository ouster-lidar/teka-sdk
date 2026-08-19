args@{
  ps,
  lib,
  stdenv,
  path,
  bash,
  replaceVars,
  writeText,
  dbus,
  pkgsStatic,
  vulkan-loader,
  sway,
  nssTools,
  p11-kit,
  adwaita-icon-theme,
  dejavu_fonts,
}:

import ../../nix/portable-firefox-wayland.nix (
  (lib.removeAttrs args [ "sway" ])
  // {
    inherit
      ps
      stdenv
      path
      bash
      replaceVars
      writeText
      dbus
      pkgsStatic
      vulkan-loader
      nssTools
      p11-kit
      adwaita-icon-theme
      dejavu_fonts
      ;
    name = "hello-firefox";
    description = "hello-firefox (Sway + Firefox on Wayland)";
    stubId = "hff";
    startUrl = "https://demos.babylonjs.com/3d-tiles/#lat=37.7749&lon=-122.4194&alt=6000";
    compositorPkg = sway;
    startLog = "starting Sway (wlroots) + Firefox on DRM with NVIDIA clients";
  }
)
