{ callPackage, teka }:
callPackage ../../../nix/apps/firefox/firefox-wayland-app.nix {
  inherit teka;
  imports = [ ./configuration.nix ];
}
