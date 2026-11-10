{ sdk, name }:

let
  source = builtins.path {
    path = sdk.templates.${name}.path;
    name = "${name}-source";
  };
  outputs = (import (source + "/flake.nix")).outputs {
    self = outputs // {
      lastModifiedDate = "20260102030405";
      shortRev = "abc1234";
    };
    teka-sdk = sdk;
  };
in
assert builtins.hasAttr name outputs.teka.apps;
{
  inherit source outputs;
}
