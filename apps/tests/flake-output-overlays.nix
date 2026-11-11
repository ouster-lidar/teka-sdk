{
  mkAppOutputs,
  nixpkgs,
  pkgs,
  version,
}:

let
  inherit (nixpkgs) lib;
  systems = [
    "x86_64-linux"
    "aarch64-linux"
  ];
  markerOverlay = _: prev: { appOverlayMarker = (prev.appOverlayMarker or 0) + 1; };
  outputsFor =
    simpleCrossApps:
    mkAppOutputs {
      self = { };
      inherit systems version simpleCrossApps;
      overlays = [ markerOverlay ];
      apps.overlay-probe.image =
        { pkgs, ps, ... }:
        pkgs.runCommand "overlay-probe"
          {
            passthru = {
              marker = pkgs.appOverlayMarker or null;
              targetSystem = pkgs.stdenv.hostPlatform.system;
              buildSystem = pkgs.stdenv.buildPlatform.system;
              sourceSystem =
                (ps.appCrate.prepareAppSource {
                  crateName = "hello-pub";
                  src = ../examples/hello-pub;
                }).src.system;
              runnerTarget = ps.tekaRunner.CARGO_BUILD_TARGET;
              expectedRunnerTarget = pkgs.stdenv.hostPlatform.rust.rustcTarget;
            };
          }
          ''
            touch "$out"
          '';
    };
  overlayApplied =
    outputs:
    !(outputs ? packages)
    && lib.all (
      buildSystem:
      lib.all (
        targetSystem:
        let
          package = outputs.teka.apps.overlay-probe.packages.${buildSystem}.${targetSystem};
          image = package.image;
        in
        image.marker == 1
        && image.targetSystem == targetSystem
        && image.buildSystem == buildSystem
        && image.sourceSystem == buildSystem
        && image.runnerTarget == image.expectedRunnerTarget
        && (builtins.elemAt package.nativeBuildInputs 1).system == buildSystem
      ) systems
    ) systems;
in
assert overlayApplied (outputsFor [ "overlay-probe" ]);
assert overlayApplied (outputsFor [ ]);
pkgs.emptyFile
