{ sdk, system }:

let
  pkgs = sdk.inputs.nixpkgs.legacyPackages.${system};
  inherit (pkgs) lib;
  # Copy only the app: no repository parent can satisfy its SDK dependency.
  detached = builtins.path {
    path = ../examples/hello-pub;
    name = "detached-rust-app";
    filter =
      path: type:
      type == "directory"
      || lib.hasSuffix ".rs" path
      || lib.hasSuffix ".toml" path
      || lib.hasSuffix ".lock" path;
  };
  appSrc = pkgs.runCommand "detached-rust-app-source" { } ''
    cp -r ${detached} "$out"
    chmod -R u+w "$out"
    echo 'const _: &str = teka::SDK_SOURCE_PROBE;' >> "$out/src/lib.rs"
  '';
  tekaSrc = sdk.packages.${system}.teka-src.overrideAttrs (old: {
    buildCommand = old.buildCommand + ''
      chmod u+w "$out/src/lib.rs"
      echo 'pub const SDK_SOURCE_PROBE: &str = "supplied-sdk";' >> "$out/src/lib.rs"
    '';
  });
  outputsFor =
    advisoryDb:
    sdk.lib.mkAppOutputs {
      self = { };
      inherit tekaSrc advisoryDb;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      simpleCrossApps = [ "hello-pub" ];
      apps.hello-pub.image =
        { ps, ... }:
        ps.appCrate.buildAppCrate (
          ps.appCrate.prepareAppSource {
            crateName = "hello-pub";
            src = appSrc;
            cargoSrc = detached;
          }
        );
    };
  outputs = outputsFor sdk.inputs.rustsecAdvisoryDb;
  native = outputs.teka.apps.hello-pub.packages.${system}.${system}.image;
  target = if system == "x86_64-linux" then "aarch64-linux" else "x86_64-linux";
  cross = outputs.teka.apps.hello-pub.packages.${system}.${target}.image;
in
assert outputs.checks.${system} ? hello-pub-audit;
assert !((outputsFor null).checks.${system} ? hello-pub-audit);
{
  rust-app-source = native;
  rust-app-source-cross = cross;
  rust-app-source-clippy = native.cargoChecks.clippy;
  rust-app-source-fmt = native.cargoChecks.fmt;
  rust-app-source-audit = native.cargoChecks.audit;
}
