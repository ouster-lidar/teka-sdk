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
  cage,
  nssTools,
  p11-kit,
  adwaita-icon-theme,
  dejavu_fonts,
}:

import ../../nix/firefox/firefox-wayland-app.nix (
  (lib.removeAttrs args [ "cage" ])
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
    name = "hello-firefox-kiosk";
    description = "hello-firefox-kiosk (Cage + Firefox kiosk on Wayland)";
    stubId = "hfk";
    startUrl = "https://webglreport.com/?v=2";
    compositorPkg = cage;
    kiosk = true;
    startLog = "starting Cage (wlroots) + Firefox kiosk on DRM with NVIDIA clients";
  }
)
