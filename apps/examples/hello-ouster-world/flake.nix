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
      apps = builtins.mapAttrs (
        system: _:
        let
          pkgs = teka-sdk.inputs.nixpkgs.legacyPackages.${system};
        in
        {
          test = {
            type = "app";
            program = pkgs.lib.getExe (
              pkgs.callPackage ./tests {
                teka = teka-sdk.packages.${system}.teka;
              }
            );
          };
        }
      ) teka-sdk.packages;
    };
}
