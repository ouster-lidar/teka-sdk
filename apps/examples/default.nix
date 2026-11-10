{ lib }:

let
  root = ./.;
  entries = builtins.readDir root;
  appNames = lib.filter (
    name: entries.${name} == "directory" && builtins.pathExists (root + "/${name}/default.nix")
  ) (lib.attrNames entries);
  appPath = name: root + "/${name}";
  apps = lib.genAttrs appNames (name: import (appPath name));
  services = lib.mapAttrs (_: app: app.image) apps;
  templateApps = lib.filter (name: builtins.pathExists (appPath name + "/flake.nix")) appNames;
  cargoApps = lib.filter (name: builtins.pathExists (appPath name + "/Cargo.toml")) appNames;
  imageCheckApps = lib.filter (
    name: builtins.pathExists (appPath name + "/tests/image-check.nix")
  ) appNames;
in
{
  inherit services apps;
  templates = lib.genAttrs templateApps (name: {
    path = appPath name;
    description = "${name} Teka app";
  });
  cargoDirs = map (name: "examples/${name}") cargoApps;
  imageChecks = lib.genAttrs imageCheckApps (name: import (appPath name + "/tests/image-check.nix"));
}
