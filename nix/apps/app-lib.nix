# Stable flake `lib` exports for cross-repo app build infrastructure.
let
  appImage = import ./app-image.nix;
in
{
  inherit (appImage)
    mkApp
    mkImageBuilder
    flakeOutputs
    ;

  mkVersion = import ./version.nix;

  appCrate = import ./app-crate.nix;

  paths = {
    erofsMkfs = ./erofs-mkfs.nix;
    coreVersionValidate = ./core-version-validate.nix;
  };
}
