{
  lib,
  runCommand,
  formats,
  craneLib,
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
  workspaceToml = (formats.toml { }).generate "workspace.toml" { workspace.package = workspace; };
  src = lib.cleanSource ../../crates/teka;
in
runCommand "teka-src"
  {
    passthru.dummySrc = craneLib.mkDummySrc {
      inherit src;
      # Resolve inherited metadata without replacing Crane's cache-busting build script.
      extraDummyScript = ''
        chmod u+w "$out/Cargo.toml"
        cat ${workspaceToml} >> "$out/Cargo.toml"
      '';
    };
  }
  ''
    cp -r ${src} "$out"
    chmod u+w "$out/Cargo.toml"
    cp ${cargoToml} "$out/Cargo.toml"
  ''
