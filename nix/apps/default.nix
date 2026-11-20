{
  nixpkgs,
  crane,
  rustsecAdvisoryDb,
  tekaFor,
}:
let
  sdkNixpkgs = nixpkgs;
  supportedSystems = [
    "x86_64-linux"
    "aarch64-linux"
  ];
in
{
  self,
  apps,
  nixpkgs ? sdkNixpkgs,
  systems ? supportedSystems,
  audit ? false,
  overlays ? [ ],
}:
let
  inherit (nixpkgs) lib;
  versionNumber = "(0|[1-9][0-9]*)";
  prereleaseId = "(${versionNumber}|[0-9]*[a-zA-Z-][0-9a-zA-Z-]*)";
  readApp =
    {
      package,
      modules ? [ ],
      imageCheck ? (_: ""),
    }:
    let
      manifest = builtins.dirOf package + "/manifest.json";
      data = lib.importJSON manifest;
      id = data.id or (throw "Missing id in ${toString manifest}");
      version = data.version or (throw "Missing version in ${toString manifest}");
    in
    assert lib.assertMsg (builtins.pathExists manifest) "Missing ${toString manifest}";
    assert lib.assertMsg (
      builtins.isString id && builtins.match "[a-z0-9][a-z0-9_-]{0,63}" id != null && id != "default"
    ) "Invalid id in ${toString manifest}";
    assert lib.assertMsg (
      builtins.isString version
      && builtins.stringLength version <= 64
      &&
        builtins.match "${versionNumber}\\.${versionNumber}\\.${versionNumber}(-${prereleaseId}(\\.${prereleaseId})*)?" version
        != null
    ) "Invalid version in ${toString manifest}";
    lib.nameValuePair id {
      inherit
        package
        modules
        imageCheck
        manifest
        version
        ;
      fullVersion = "${version}+${buildId}";
    };
  appsById = builtins.listToAttrs (map readApp apps);
  appNames = lib.attrNames appsById;
  artifactBase = id: target: "${id}-${target}-v${appsById.${id}.fullVersion}";
  buildId =
    let
      shortRev = self.shortRev or "";
      dirtyRev = builtins.replaceStrings [ "-dirty" ] [ "" ] (self.dirtyShortRev or "");
      rev =
        if shortRev != "" && shortRev != "0000000" then
          shortRev
        else if dirtyRev != "" then
          dirtyRev
        else
          "0000000";
      hash = builtins.substring 0 7 (builtins.hashString "sha256" (self.narHash or ""));
      id = "${self.lastModifiedDate or "19700101000000"}.g${rev}.n${hash}";
    in
    assert lib.assertMsg (
      builtins.match "[0-9]{14}\\.g[0-9a-f]+\\.n[0-9a-f]{7}" id != null
    ) "Malformed app build ID '${id}'";
    id;
  forSystems = lib.genAttrs systems;
  waylandOverlay = import ./overlays/wayland-compositors-overlay.nix;
  crossOverlay = import ./overlays/cross.nix;

  packageSets = forSystems (
    buildSystem:
    forSystems (
      targetSystem:
      let
        isCross = buildSystem != targetSystem;
      in
      import nixpkgs (
        {
          localSystem = buildSystem;
          config.allowUnfree = true;
          overlays = overlays ++ [ waylandOverlay ] ++ lib.optional isCross crossOverlay;
        }
        // lib.optionalAttrs isCross { crossSystem = targetSystem; }
      )
    )
  );

  packageFor =
    id: buildSystem: targetSystem:
    let
      pkgs = packageSets.${buildSystem}.${targetSystem};
      app = appsById.${id};
      inherit (app) version;
      basename = artifactBase id targetSystem;
      artifact = "${basename}.raw";
      image = lib.makeOverridable (
        {
          modules ? [ ],
        }:
        let
          teka = import ./app-scope.nix {
            name = id;
            inherit pkgs artifact buildId;
            inherit (app) manifest version;
            modules = app.modules ++ modules;
            advisoryDb = if audit then rustsecAdvisoryDb else null;
            craneLib = crane.mkLib pkgs;
          };
        in
        (pkgs.newScope { inherit teka; }) app.package { }
      ) { };
      tpkFileName = "${basename}.tpk";
      arch = lib.removeSuffix "-linux" targetSystem;
    in
    pkgs.buildPackages.runCommand "${id}-${targetSystem}-tpk"
      {
        pname = id;
        inherit version;
        __structuredAttrs = true;
        # Package members contain the signed image and its complete runtime closure.
        unsafeDiscardReferences.out = true;
        nativeBuildInputs = [
          pkgs.buildPackages.jq
          (tekaFor buildSystem)
        ];
        passthru = { inherit image tpkFileName; };
      }
      ''
        raw=${lib.escapeShellArg "${image}/${artifact}"}
        jq -n \
          --arg arch ${lib.escapeShellArg arch} \
          --arg version ${lib.escapeShellArg app.fullVersion} \
          --arg id ${lib.escapeShellArg id} \
          --arg root_hash "$(cat ${lib.escapeShellArg "${image}/${basename}.roothash"})" \
          '{format:1,arch:$arch,components:[{type:"app",id:$id,version:$version,root_hash:$root_hash,image:{path:($id+".raw")}}]}' >manifest.json
        mkdir -p "$out"
        teka tpk pack --manifest manifest.json \
          --member ${lib.escapeShellArg "${id}.raw"}="$raw" \
          --private-key ${./dev-keys/apps.key} --certificate ${./dev-keys/apps.pem} \
          --output "$out/${tpkFileName}"
      '';

  appPackages = lib.genAttrs appNames (
    id: forSystems (buildSystem: forSystems (packageFor id buildSystem))
  );

  defaultTarget = if lib.elem "aarch64-linux" systems then "aarch64-linux" else lib.head systems;
  packagesFor =
    buildSystem:
    let
      packages = lib.mapAttrs (
        _: byBuilder:
        let
          targets = byBuilder.${buildSystem};
        in
        targets.${defaultTarget}.overrideAttrs (old: {
          passthru = old.passthru // {
            inherit targets;
          };
        })
      ) appPackages;
    in
    packages
    // lib.optionalAttrs (lib.length appNames == 1) {
      default = packages.${lib.head appNames};
    };

  mkRawCheck = import ./image-check.nix;

  checksFor =
    buildSystem:
    let
      pkgs = packageSets.${buildSystem}.${buildSystem};
      imageCheck =
        id: targetSystem:
        let
          name = if buildSystem == targetSystem then id else "${id}-cross-${targetSystem}";
          app = appsById.${id};
          image = appPackages.${id}.${buildSystem}.${targetSystem}.image;
        in
        lib.nameValuePair name (mkRawCheck {
          inherit pkgs targetSystem buildId;
          inherit (app) manifest imageCheck;
          checkName = "app-image-check-${name}";
          serviceName = id;
          imageVersion = app.version;
          raw = "${image}/${artifactBase id targetSystem}.raw";
        });
      entries = lib.concatMap (id: map (imageCheck id) systems) appNames;
      checks = builtins.listToAttrs entries;
    in
    assert lib.assertMsg (
      lib.length entries == lib.length (lib.attrNames checks)
    ) "Duplicate app check names for ${buildSystem}";
    checks;
in
assert lib.assertMsg (builtins.length apps == builtins.length appNames) "Duplicate app ID";
assert lib.assertMsg (
  systems != [ ] && lib.all (system: lib.elem system supportedSystems) systems
) "Teka app builders and targets must be x86_64-linux or aarch64-linux";
assert lib.assertMsg (lib.unique systems == systems) "Duplicate Teka app systems";
{
  teka.lib.linkFarm = system: nixpkgs.legacyPackages.${system}.linkFarm;
  teka.apps = lib.genAttrs appNames (id: {
    path =
      let
        root = toString (self.outPath or "");
        dir = toString (builtins.dirOf appsById.${id}.package);
        prefix = root + "/";
      in
      if root == "" then
        null
      else if dir == root then
        "."
      else if lib.hasPrefix prefix dir then
        builtins.unsafeDiscardStringContext (lib.removePrefix prefix dir)
      else
        null;
  });
  packages =
    forSystems packagesFor
    // lib.optionalAttrs (lib.elem "aarch64-linux" systems) {
      aarch64-darwin = packagesFor "aarch64-linux";
    };
  checks = forSystems checksFor;
}
