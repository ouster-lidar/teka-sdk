{ sdk, name }:

let
  source = builtins.path {
    path = sdk.templates.${name}.path;
    name = "${name}-source";
  };
  outputs = (import (source + "/flake.nix")).outputs {
    self = outputs // {
      outPath = source;
    };
    teka-sdk = sdk;
  };
in
assert builtins.hasAttr name outputs.teka.apps;
{
  inherit source outputs;
}
