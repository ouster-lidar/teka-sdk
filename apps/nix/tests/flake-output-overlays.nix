{
  appLib,
  crane,
  nixpkgs,
  pkgs,
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
      lib.all (name: outputs.packages.${system}.${name}.marker == "applied") (
        lib.filter (name: name != "default") (lib.attrNames outputs.packages.${system})
      )
    ) systems;
in
assert overlayApplied (outputsFor [ "overlay-probe" ]);
assert overlayApplied (outputsFor [ ]);
pkgs.runCommand "flake-output-overlays-check" { } ''
  touch "$out"
''
