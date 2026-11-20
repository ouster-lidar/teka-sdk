{
  name,
  modules,
  pkgs,
  craneLib,
  version,
  buildId,
  artifact,
  manifest,
  advisoryDb ? null,
}:
let
  wayland = pkgs.callPackage ./wayland/app-support.nix {
    dbus = pkgs.dbus-session;
    inherit name;
  };

  buildImage = pkgs.callPackage ./image-builder.nix {
    inherit (teka) appDir;
    inherit
      name
      version
      buildId
      artifact
      manifest
      ;
  };

  teka.appId = name;
  teka.appDir = "usr/lib/teka/app";
  teka.lib = {
    buildRustApp = pkgs.callPackage ./app-crate.nix { inherit craneLib advisoryDb; };
    inherit wayland;
    tekaRunner = pkgs.callPackage ../packages/teka-runner.nix { inherit craneLib; };
    mkAppImage = import ./app-recipe.nix {
      inherit
        pkgs
        teka
        modules
        buildImage
        ;
    };
  };
in
teka
