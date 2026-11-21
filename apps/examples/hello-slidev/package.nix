{
  lib,
  pkgs,
  teka,
}:
let
  inherit (pkgs) buildPackages;
  program = teka.lib.buildRustApp { srcDir = ./.; };

  slidevSrc = builtins.path {
    path = ./slidev;
    name = "slidev";
    filter =
      path: _:
      !lib.elem (baseNameOf path) [
        "node_modules"
        "dist"
        "ouster"
      ];
  };

  ousterAssets = import ./ouster-assets.nix {
    inherit (buildPackages) fetchurl;
  };

  slidevSrcWithAssets = buildPackages.runCommand "hello-slidev-slidev-src" { } ''
    mkdir -p $out/ouster
    cp -r ${slidevSrc}/. $out/
    cp ${ousterAssets.hero} $out/ouster/hero-physical-ai.png
    cp ${ousterAssets.logo} $out/ouster/logo.svg
    cp ${ousterAssets.sensorLineup} $out/ouster/sensor-lineup.png
    cp ${ousterAssets.robotics} $out/ouster/robotics.png
    cp ${ousterAssets.smartInfrastructure} $out/ouster/smart-infrastructure.png
    cp ${ousterAssets.marketsHero} $out/ouster/markets-hero.png
    cp ${ousterAssets.sensorOs1} $out/ouster/sensor-os1.png
  '';

  # Build the static deck on the build platform to keep Node out of the target closure.
  slidevDist = buildPackages.buildNpmPackage {
    pname = "hello-slidev-slidev";
    version = "1.0.0";
    src = slidevSrcWithAssets;
    npmDepsHash = "sha256-ixMJ/po2V9D0oi5FOj81tEf9HkXWQ7oNejsy+9+1ERA=";
    dontNpmBuild = true;

    buildPhase = "npm run build";
    installPhase = ''
      cp -r dist $out
      cp -r ouster $out/ouster
    '';

    preferLocalBuild = true;
  };
in
teka.lib.mkAppImage {
  services.main = {
    inherit program;
    runtime.managed = true;
    systemd = {
      after = [ "network.target" ];
      wants = [ "network.target" ];
      environment.DECK_ROOT = "${slidevDist}";
      serviceConfig = {
        ProtectSystem = false;
        ProtectHome = false;
        StateDirectory = "%N";
        StateDirectoryMode = "0750";
        Restart = "on-failure";
      };
    };
  };
  image.packages = [
    (pkgs.linkFarm "hello-slidev-web" { "${teka.appDir}/web" = ./web; })
    pkgs.teka-app-api-js
  ];
}
