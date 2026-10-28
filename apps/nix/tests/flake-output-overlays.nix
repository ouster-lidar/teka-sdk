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
  markerOverlay = _: _: { appOverlayMarker = "applied"; };
  outputsFor =
    simpleCrossServices:
    appLib.flakeOutputs {
      inherit
        crane
        systems
        version
        tekaFor
        simpleCrossServices
        ;
      appOverlays = [ markerOverlay ];
      nixpkgs = nixpkgs // {
        legacyPackages = lib.genAttrs systems (
          system:
          import nixpkgs {
            inherit system;
            overlays = [ markerOverlay ];
          }
        );
      };
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
        name: !(lib.hasSuffix "-tpk" name) && outputs.packages.${system}.${name}.image.marker == "applied"
      ) (lib.filter (name: name != "default") (lib.attrNames outputs.packages.${system}))
    ) systems;
in
assert overlayApplied (outputsFor [ "overlay-probe" ]);
assert overlayApplied (outputsFor [ ]);
pkgs.runCommand "flake-output-overlays-check" { } ''
  touch "$out"
''
