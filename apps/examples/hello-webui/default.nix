{
  image =
    { ps, replaceVars }:
    let
      app = ps.appCrate;
      name = "hello-webui";
      webTree = app.appWebTree {
        inherit name;
        webSrc = ./web;
        webFiles = [
          "demo.html"
          "demo.js"
          "demo.css"
        ];
      };
      appManifest = replaceVars ./manifest.json { inherit (ps) imageVersion; };
    in
    ps.makeAppImage {
      inherit name;
      unit = ./hello-webui.service;
      copyFiles = app.appManifestCopyFiles {
        inherit name appManifest webTree;
      };
    };
}
