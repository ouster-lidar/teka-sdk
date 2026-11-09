{
  appLib,
  crane,
  nixpkgs,
  pkgs,
  tekaFor,
  version,
}:

let
  inherit (nixpkgs) lib;
  systems = [
    "x86_64-linux"
    "aarch64-linux"
    "aarch64-darwin"
  ];
  markerOverlay = _: prev: { appOverlayMarker = (prev.appOverlayMarker or 0) + 1; };
  outputsFor =
    simpleCrossServices:
    appLib.flakeOutputs {
      inherit
        crane
        systems
        version
        tekaFor
        simpleCrossServices
        nixpkgs
        ;
      appOverlays = [ markerOverlay ];
      services.overlay-probe =
        { pkgs, ... }:
        pkgs.runCommand "overlay-probe"
          {
            passthru.marker = pkgs.appOverlayMarker or null;
          }
          ''
            touch "$out"
          '';
    };
  overlayApplied =
    outputs:
    lib.all (
      system:
      lib.all (
        name: !(lib.hasSuffix "-tpk" name) && outputs.packages.${system}.${name}.image.marker == 1
      ) (lib.filter (name: name != "default") (lib.attrNames outputs.packages.${system}))
    ) systems;
in
assert overlayApplied (outputsFor [ "overlay-probe" ]);
assert overlayApplied (outputsFor [ ]);
pkgs.runCommand "flake-output-overlays-check" { } ''
  touch "$out"
''
