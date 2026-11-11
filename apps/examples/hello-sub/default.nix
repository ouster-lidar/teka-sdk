{
  image =
    {
      ps,
      lib,
      replaceVars,
    }:

    let
      app = ps.appCrate;

      name = "hello-sub";

      helloSubSrc = app.appRustSrc { srcDir = ./.; };

      helloSubBin = app.buildAppCrate (
        (app.prepareAppSource {
          crateName = "hello-sub";
          src = helloSubSrc;
        })
        // {
          meta = {
            mainProgram = "hello-sub";
          };
        }
      );

      jp = app.appHostUnitDefaults;

      webTree = app.appWebTree {
        inherit name;
        webSrc = ./web;
        webFiles = [
          "index.html"
          "hello-sub.html"
          "hello-sub.js"
          "hello-sub.css"
        ];
      };

      appManifest = replaceVars ./manifest.json {
        imageVersion = ps.imageVersion;
      };
    in

    ps.makeAppImage {
      inherit name;
      cargoChecks = helloSubBin.passthru.cargoChecks;
      unit = replaceVars ./hello-sub.service {
        appBackendUnitLines = ps.appBackendUnitLines;
        appNotifyServiceLines = ps.appNotifyServiceLines;
        binPath = lib.makeBinPath [ helloSubBin ];
        appBackendServiceLines = ps.appBackendServiceLines;
        serviceFragment = jp.serviceFragment;
        program = lib.getExe helloSubBin;
      };
      copyFiles = app.appManifestCopyFiles {
        inherit name appManifest webTree;
      };
      runtimeClosures = [ helloSubBin ];
    };
}
