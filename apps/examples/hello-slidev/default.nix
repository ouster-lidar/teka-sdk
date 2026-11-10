{
  image =
    {
      ps,
      pkgs,
      lib,
      replaceVars,
    }:

    let
      app = ps.appCrate;

      name = "hello-slidev";

      helloSlidevSrc = app.appRustSrc { srcDir = ./.; };

      helloSlidevBin = app.buildAppCrate {
        crateName = "hello-slidev";
        filteredSrc = helloSlidevSrc;
        meta = {
          mainProgram = "hello-slidev";
        };
      };

      slidevSrc = builtins.path {
        path = ./slidev;
        name = "slidev";
        filter =
          path: _:
          let
            base = baseNameOf path;
          in
          !(base == "node_modules" || base == "dist" || base == "ouster");
      };

      ousterAssets = import ./ouster-assets.nix {
        fetchurl = pkgs.buildPackages.fetchurl;
      };

      slidevSrcWithAssets = pkgs.buildPackages.runCommand "hello-slidev-slidev-src" { } ''
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

      # Slidev is static HTML/JS/CSS — build on the build platform (x86_64 when
      # cross-compiling to aarch64) so we never pull node/icu/simdutf into the
      # target closure or remote aarch64 builders.
      slidevDist = pkgs.buildPackages.buildNpmPackage {
        pname = "hello-slidev-slidev";
        version = "1.0.0";
        src = slidevSrcWithAssets;

        npmDepsHash = "sha256-ixMJ/po2V9D0oi5FOj81tEf9HkXWQ7oNejsy+9+1ERA=";

        # Slidev build is custom; npm deps are fetched via npmDepsHash above.
        dontNpmBuild = true;

        buildPhase = "npm run build";
        installPhase = ''
          cp -r dist $out
          cp -r ouster $out/ouster
        '';

        preferLocalBuild = true;
      };

      jp = app.appHostUnitDefaults;

      webTree = app.appWebTree {
        inherit name;
        webSrc = ./web;
        webFiles = [
          "slides.html"
          "slides.js"
          "hello-slidev.css"
        ];
      };

      appManifest = replaceVars ./manifest.json {
        imageVersion = ps.imageVersion;
      };
    in

    ps.makeAppImage {
      inherit name;
      cargoChecks = helloSlidevBin.passthru.cargoChecks;
      unit = replaceVars ./hello-slidev.service {
        appBackendUnitLines = ps.appBackendUnitLines;
        appNotifyServiceLines = ps.appNotifyServiceLines;
        binPath = lib.makeBinPath [ helloSlidevBin ];
        appBackendServiceLines = ps.appBackendServiceLines;
        serviceFragment = jp.serviceFragment;
        program = lib.getExe helloSlidevBin;
      };
      copyFiles =
        app.appManifestCopyFiles {
          inherit name appManifest webTree;
        }
        ++ [
          {
            path = "usr/lib/hello-slidev/deck";
            source = slidevDist;
          }
        ];
      runtimeClosures = [ helloSlidevBin ];
    };
}
