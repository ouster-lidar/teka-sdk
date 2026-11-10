{
  description = "hello-ouster-world Teka app";

  inputs.teka-sdk.url = "github:ouster-lidar/teka-sdk/main";

  outputs =
    { self, teka-sdk, ... }:
    let
      builds = teka-sdk.lib.mkAppOutputs {
        inherit self;
        apps.hello-ouster-world = import ./.;
      };
    in
    builds
    // {
      packages = builtins.mapAttrs (
        system: packages:
        packages
        // {
          test = teka-sdk.inputs.app-nixpkgs.legacyPackages.${system}.callPackage ./tests {
            teka = teka-sdk.packages.${system}.teka;
          };
        }
      ) builds.packages;
    };
}
