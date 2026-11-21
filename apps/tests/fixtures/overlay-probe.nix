{ pkgs, teka }:
(teka.lib.mkAppImage {
  services.main.systemd.serviceConfig.ExecStart = "/bin/true";
}).overrideAttrs
  (_: {
    passthru = {
      marker = pkgs.appOverlayMarker or null;
      targetSystem = pkgs.stdenv.hostPlatform.system;
      buildSystem = pkgs.stdenv.buildPlatform.system;
      runnerTarget = teka.lib.tekaRunner.CARGO_BUILD_TARGET;
      expectedRunnerTarget = pkgs.stdenv.hostPlatform.rust.rustcTarget;
    };
  })
