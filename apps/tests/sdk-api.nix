{ sdk, system }:

let
  pkgs = sdk.inputs.nixpkgs.legacyPackages.${system};
in
assert
  (sdk.lib.mkAppOutputs {
    self = { };
    apps.unbuilt = throw "app registration forced an app value";
  })
    ? teka.apps.unbuilt;
assert
  !(
    (sdk.lib.mkAppOutputs {
      self = { };
      apps = { };
    })
      ? packages
  );
assert
  builtins.attrNames sdk.packages.${system} == [
    "default"
    "portable"
    "teka"
    "teka-runner"
    "teka-src"
  ];
assert
  builtins.attrNames sdk.teka.apps
  == builtins.attrNames (import ../examples { inherit (pkgs) lib; }).apps;
assert
  builtins.attrNames sdk.teka.apps.hello-world.packages == [
    "aarch64-linux"
    "x86_64-linux"
  ];
assert sdk.templates.default.path == sdk.templates.hello-world.path;
pkgs.emptyFile
