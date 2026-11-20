{ lib }:

let
  root = ./.;
  entries = builtins.readDir root;
  appNames = lib.filter (
    name: entries.${name} == "directory" && builtins.pathExists (appPath name + "/package.nix")
  ) (lib.attrNames entries);
  appPath = name: root + "/${name}";
  apps = map (
    name:
    let
      package = appPath name + "/package.nix";
      check = appPath name + "/tests/image-check.nix";
    in
    { inherit package; } // lib.optionalAttrs (builtins.pathExists check) { imageCheck = import check; }
  ) appNames;
  templateApps = lib.filter (name: builtins.pathExists (appPath name + "/flake.nix")) appNames;
  cargoApps = lib.filter (name: builtins.pathExists (appPath name + "/Cargo.toml")) appNames;
in
{
  inherit apps;
  templates = lib.genAttrs templateApps (name: {
    path = appPath name;
    description = "${name} Teka app";
  });
  cargoDirs = map (name: "apps/examples/${name}") cargoApps;
}
