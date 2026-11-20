{ sdk, system }:

let
  args = {
    self = { };
    systems = [
      "x86_64-linux"
      "aarch64-linux"
    ];
    apps = [ { package = ./fixtures/rust-app.nix; } ];
  };
  inherit (sdk.inputs.nixpkgs) lib;
  outputs = sdk.lib.mkAppOutputs (args // { audit = true; });
  native = outputs.packages.${system}.packaging-probe.targets.${system}.image;
  target = if system == "x86_64-linux" then "aarch64-linux" else "x86_64-linux";
  cross = outputs.packages.${system}.packaging-probe.targets.${target}.image;
  withoutAudit =
    (sdk.lib.mkAppOutputs args).packages.${system}.packaging-probe.targets.${system}.image.app;
in
assert native.app.tests ? audit;
assert !(withoutAudit.tests ? audit);
{
  rust-app-source = native.app;
  rust-app-source-cross = cross.app;
}
// lib.mapAttrs' (name: lib.nameValuePair "rust-app-${name}") native.app.tests
