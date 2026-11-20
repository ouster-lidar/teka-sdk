{ lib, teka }:
let
  # Copy only the app so the SDK dependency must be supplied by the builder.
  src = lib.cleanSourceWith {
    src = ../../examples/hello-pub;
    name = "detached-rust-app";
    filter =
      path: type:
      type == "directory"
      || lib.hasSuffix ".rs" path
      || lib.hasSuffix ".toml" path
      || lib.hasSuffix ".lock" path;
  };
  app = teka.lib.buildRustApp {
    inherit src;
    meta.mainProgram = "hello-pub";
  };
in
(teka.lib.mkAppImage {
  services.main.program = app;
}).overrideAttrs
  (_: {
    passthru = {
      inherit app;
    };
  })
