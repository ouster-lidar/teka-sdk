{
  appLib,
  app-nixpkgs,
  crane,
  ouster-sdk,
  tekaFor,
}:

{
  self,
  apps ? { },
  nixpkgs ? app-nixpkgs,
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
