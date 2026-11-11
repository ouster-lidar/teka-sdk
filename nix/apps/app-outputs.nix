{
  appLib,
  nixpkgs,
  crane,
  ouster-sdk,
  tekaFor,
}:

let
  defaultNixpkgs = nixpkgs;
in

{
  self,
  apps ? { },
  nixpkgs ? defaultNixpkgs,
  systems ? [
    "x86_64-linux"
    "aarch64-linux"
    "aarch64-darwin"
  ],
  version ? appLib.mkVersion {
    inherit self;
    inherit (nixpkgs) lib;
  },
  ousterSdkSrc ? ouster-sdk,
  appOverlays ? [ ],
  imageChecks ? { },
  simpleCrossServices ? [ ],
  services ? nixpkgs.lib.mapAttrs (_: app: app.image) apps,
}:
appLib.flakeOutputs {
  inherit
    nixpkgs
    crane
    systems
    services
    version
    ousterSdkSrc
    appOverlays
    imageChecks
    simpleCrossServices
    tekaFor
    ;
}
// nixpkgs.lib.optionalAttrs (apps != { }) {
  teka.apps = apps;
}
