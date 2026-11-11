{
  lib,
  runCommand,
  formats,
}:
let
  workspace = (builtins.fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package;
  manifest = builtins.fromTOML (builtins.readFile ../../crates/teka/Cargo.toml);
  standalone = manifest // {
    package = lib.mapAttrs (
      name: value: if builtins.isAttrs value && value.workspace or false then workspace.${name} else value
    ) manifest.package;
  };
  cargoToml = (formats.toml { }).generate "Cargo.toml" standalone;
in
runCommand "teka-src" { } ''
  cp -r ${lib.cleanSource ../../crates/teka} "$out"
  chmod u+w "$out/Cargo.toml"
  cp ${cargoToml} "$out/Cargo.toml"
''
