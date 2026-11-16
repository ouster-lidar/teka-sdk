# Secondary nixpkgs eval for firefox-bin (unfree + optional cross). Shared by
# hello-firefox and hello-firefox-kiosk. Uses the same PipeWire cross overlay as
# the shared cross overlay (see nix/apps/overlays/cross.nix).
{
  lib,
  stdenv,
  path,
}:
let
  crossOverlay = import ../overlays/cross.nix;

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
      overlays = [ crossOverlay ];
    }
  );
in
{
  inherit mozillaBinUnfreePredicate;
  firefox-bin = firefoxPkgs.firefox-bin;
}
