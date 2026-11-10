{ sdk, system }:

let
  pkgs = sdk.inputs.app-nixpkgs.legacyPackages.${system};
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
      services.unbuilt = throw "metadata lookup forced a service value";
    })
      ? teka
  );
assert sdk.templates.default.path == sdk.templates.hello-world.path;
pkgs.emptyFile
