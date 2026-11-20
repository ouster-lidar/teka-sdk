{
  mkAppOutputs,
  nixpkgs,
  pkgs,
}:

let
  inherit (nixpkgs) lib;
  systems = [
    "x86_64-linux"
    "aarch64-linux"
  ];
  markerOverlay = _: prev: { appOverlayMarker = (prev.appOverlayMarker or 0) + 1; };
  outputs = mkAppOutputs {
    self = { };
    inherit systems;
    overlays = [ markerOverlay ];
    apps = [
      {
        package = ./fixtures/overlay-probe.nix;
        imageCheck =
          { pkgs, ... }:
          assert pkgs.appOverlayMarker == 1;
          "";
      }
    ];
  };
  overlayApplied =
    outputs.packages.aarch64-darwin.default.drvPath
    == outputs.packages.aarch64-linux.packaging-probe.targets.aarch64-linux.drvPath
    && lib.all (
      buildSystem:
      let
        packages = outputs.packages.${buildSystem};
        inherit (packages.packaging-probe) targets;
      in
      packages.default.drvPath == targets.aarch64-linux.drvPath
      && packages.packaging-probe.drvPath == targets.aarch64-linux.drvPath
      && lib.all (
        targetSystem:
        let
          package = targets.${targetSystem};
          image = package.image;
          checkName =
            if buildSystem == targetSystem then "packaging-probe" else "packaging-probe-cross-${targetSystem}";
        in
        builtins.isString outputs.checks.${buildSystem}.${checkName}.drvPath
        && image.marker == 1
        && image.targetSystem == targetSystem
        && image.buildSystem == buildSystem
        && image.runnerTarget == image.expectedRunnerTarget
        && (builtins.elemAt package.nativeBuildInputs 1).system == buildSystem
      ) systems
    ) systems;
in
assert overlayApplied;
assert
  (outputs.teka.lib.linkFarm pkgs.stdenv.hostPlatform.system "app-output-probe" [ ]).drvPath
  == (pkgs.linkFarm "app-output-probe" [ ]).drvPath;
pkgs.emptyFile
