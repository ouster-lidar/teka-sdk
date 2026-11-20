{
  description = "hello-ouster-world Teka app";

  inputs.teka-sdk.url = "github:ouster-lidar/teka-sdk/main";

  outputs =
    { self, teka-sdk, ... }:
    teka-sdk.lib.mkAppOutputs {
      inherit self;
      apps = [
        {
          package = ./package.nix;
          imageCheck = import ./tests/image-check.nix;
        }
      ];
    }
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
                inherit (teka-sdk.packages.${system}) teka;
              }
            );
          };
        }
      ) teka-sdk.packages;
    };
}
