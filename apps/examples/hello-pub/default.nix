{
  image =
    {
      ps,
      lib,
      replaceVars,
    }:

    let
      app = ps.appCrate;

      name = "hello-pub";

      helloPubSrc = app.appRustSrc { srcDir = ./.; };

      helloPubBin = app.buildAppCrate {
        crateName = "hello-pub";
        filteredSrc = helloPubSrc;
        meta = {
          mainProgram = "hello-pub";
        };
      };

      jp = app.appHostUnitDefaults;

      webTree = app.appWebTree {
        inherit name;
        webSrc = ./web;
        webFiles = [
          "index.html"
          "hello-pub.html"
          "hello-pub.js"
          "hello-pub.css"
        ];
      };

      appManifest = replaceVars ./manifest.json {
        imageVersion = ps.imageVersion;
      };
    in

    ps.makeAppImage {
      inherit name;
      cargoChecks = helloPubBin.passthru.cargoChecks;
      unit = replaceVars ./hello-pub.service {
        appBackendUnitLines = ps.appBackendUnitLines;
        appNotifyServiceLines = ps.appNotifyServiceLines;
        binPath = lib.makeBinPath [ helloPubBin ];
        appBackendServiceLines = ps.appBackendServiceLines;
        serviceFragment = jp.serviceFragment;
        program = lib.getExe helloPubBin;
      };
      copyFiles = app.appManifestCopyFiles {
        inherit name appManifest webTree;
      };
      runtimeClosures = [ helloPubBin ];
    };
}
