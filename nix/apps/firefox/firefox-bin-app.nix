# Secondary nixpkgs eval for firefox-bin (unfree + optional cross). Shared by
# hello-firefox and hello-firefox-kiosk. Uses the same PipeWire cross overlay as
# the flake’s app `crossOverlay` (see nix/overlays/pipewire-cross-no-modemmanager-overlay.nix).
{ lib, stdenv, path }:
let
  pipewireCrossNoModemmanagerOverlay = import ../overlays/pipewire-cross-no-modemmanager-overlay.nix;

  mozillaBinUnfreePredicate =
    pkg:
    builtins.elem (pkg.pname or "") [
      "firefox-bin"
      "firefox-bin-unwrapped"
    ];

  firefoxPkgs = import path (
    {
      localSystem = {
        system = stdenv.buildPlatform.system;
      };
      config.allowUnfreePredicate = mozillaBinUnfreePredicate;
    }
    // lib.optionalAttrs (stdenv.hostPlatform.system != stdenv.buildPlatform.system) {
      crossSystem = {
        system = stdenv.hostPlatform.system;
      };
      overlays = [ pipewireCrossNoModemmanagerOverlay ];
    }
  );
in
{
  inherit mozillaBinUnfreePredicate;
  firefox-bin = firefoxPkgs.firefox-bin;
}
