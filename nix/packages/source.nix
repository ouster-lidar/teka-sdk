{
  lib,
  craneLib,
  cli ? false,
}:
lib.cleanSourceWith {
  src = ../../.;
  filter =
    path: type:
    let
      relative = lib.removePrefix "${toString ../../.}/" path;
      member =
        lib.hasPrefix "crates/teka/" relative || (cli && lib.hasPrefix "crates/teka-cli/" relative);
    in
    !(lib.elem "target" (lib.splitString "/" relative))
    && (
      (type == "directory" && (relative == "crates" || lib.hasPrefix "crates/" relative))
      || lib.elem relative [
        "Cargo.toml"
        "Cargo.lock"
        "crates/teka/Cargo.toml"
        "crates/teka-cli/Cargo.toml"
      ]
      || (
        member
        && (
          craneLib.filterCargoSources path type
          || (cli && lib.hasSuffix "/src/build_vm_network.swift" path)
          || lib.any (suffix: lib.hasSuffix suffix path) [
            ".nix"
            ".json"
            ".py"
          ]
        )
      )
    );
}
