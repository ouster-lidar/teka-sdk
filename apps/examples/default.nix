{ lib }:

let
  root = ./.;
  entries = builtins.readDir root;
  appNames = lib.filter (
    name: entries.${name} == "directory" && builtins.pathExists (root + "/${name}/default.nix")
  ) (lib.attrNames entries);
  appPath = name: root + "/${name}";
  cargoApps = lib.filter (name: builtins.pathExists (appPath name + "/Cargo.toml")) appNames;
  imageCheckApps = lib.filter (
    name: builtins.pathExists (appPath name + "/tests/image-check.nix")
  ) appNames;
in
{
  services = lib.genAttrs appNames appPath;
  cargoDirs = map (name: "examples/${name}") cargoApps;
  imageChecks = lib.genAttrs imageCheckApps (name: import (appPath name + "/tests/image-check.nix"));
}
