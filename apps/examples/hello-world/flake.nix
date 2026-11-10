{
  description = "hello-world Teka app";

  inputs.teka-sdk.url = "github:ouster-lidar/teka-sdk/main";

  outputs =
    { self, teka-sdk, ... }:
    teka-sdk.lib.mkAppOutputs {
      inherit self;
      apps.hello-world = import ./.;
    };
}
