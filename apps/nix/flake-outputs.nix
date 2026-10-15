{ mkApp }:

{
  nixpkgs,
  crane,
  systems,
  services,
  version,
  advisoryDb ? null,
  ousterSdkSrc ? null,
  ousterSdkNix ? ./ouster/ouster-sdk.nix,
  exampleAppsPkgsFor ? null,
  # hello-world/hello-http cross builds use a plain nixpkgs import; the rest
  # need heavyCrossOverlay (PipeWire, onnxruntime, openblas quirks).
  simpleCrossServices ? [
    "hello-world"
    "hello-http"
    "hello-llm"
    "nvidia-devtools"
  ],
  appOverlays ? [ ],
}:
let
  inherit (nixpkgs) lib;
  inherit (version) appArtifactRawName imageVersion buildId;

  waylandCompositorsOverlay = import ./overlays/wayland-compositors-overlay.nix;
  appPkgsFor = system: nixpkgs.legacyPackages.${system}.extend waylandCompositorsOverlay;

  allServiceNames = lib.sort lib.lessThan (lib.attrNames services);
  heavyCrossServices = lib.filter (n: !(lib.elem n simpleCrossServices)) allServiceNames;
  # Portable images target Linux only; other `systems` members (e.g. aarch64-darwin) cross to these.
  linuxSystems = lib.filter (lib.hasSuffix "-linux") systems;

  hostCpu =
    host:
    if lib.hasSuffix "-linux" host then
      lib.removeSuffix "-linux" host
    else if lib.hasSuffix "-darwin" host then
      lib.removeSuffix "-darwin" host
    else
      null;

  defaultLinuxTarget =
    host:
    let
      cpu = hostCpu host;
    in
    if cpu == null then null else "${cpu}-linux";

  # Cross builds: PipeWire + ModemManager cross quirk — see
  # nix/overlays/pipewire-cross-no-modemmanager-overlay.nix (shared with any separate
  # nixpkgs import that must match this flake’s cross package set).
  heavyCrossOverlay =
    final: prev:
    (import ./overlays/pipewire-cross-no-modemmanager-overlay.nix) final prev
    // (import ./overlays/wayland-compositors-overlay.nix) final prev
    // {
      onnxruntime = prev.onnxruntime.overrideAttrs (old: {
        cmakeFlags =
          let
            flags = old.cmakeFlags or [ ];
          in
          (builtins.filter (
            f: !(builtins.isString f && prev.lib.strings.hasInfix "ONNX_CUSTOM_PROTOC_EXECUTABLE" f)
          ) flags)
          ++ [
            (prev.lib.cmakeFeature "ONNX_CUSTOM_PROTOC_EXECUTABLE" (
              prev.lib.getExe prev.buildPackages.protobuf
            ))
          ];
      });
      openblas =
        if
          final.stdenv.hostPlatform.isLinux
          && final.stdenv.hostPlatform.isAarch64
          && final.stdenv.hostPlatform.system != final.stdenv.buildPlatform.system
        then
          prev.openblas.override { dynamicArch = false; }
        else
          prev.openblas;
    };

  mkCross =
    host: target:
    import nixpkgs {
      localSystem = {
        system = host;
      };
      crossSystem = {
        system = target;
      };
      config.allowUnfree = true;
      overlays = [ heavyCrossOverlay ];
    };

  perHostPackages =
    host:
    let
      pkgs = appPkgsFor host;
      callService = pkgs.callPackage;
      crossTargets = lib.filter (t: t != host) linuxSystems;
      nativeByName =
        if lib.hasSuffix "-linux" host then
          lib.genAttrs allServiceNames (
            n:
            callService services.${n} {
              ps = mkApp {
                craneLib = crane.mkLib pkgs;
                inherit
                  pkgs
                  version
                  advisoryDb
                  ousterSdkSrc
                  ousterSdkNix
                  ;
                exampleAppsPkgs = if exampleAppsPkgsFor != null then exampleAppsPkgsFor host else null;
              };
            }
          )
        else
          { };

      crossPackages = lib.listToAttrs (
        lib.concatMap (
          n:
          map (target: {
            name = "${n}-cross-${target}";
            value =
              let
                crossPkgs =
                  if lib.elem n heavyCrossServices then
                    mkCross host target
                  else
                    import nixpkgs {
                      localSystem = {
                        system = host;
                      };
                      crossSystem = {
                        system = target;
                      };
                      config.allowUnfree = true;
                      overlays = appOverlays ++ [ waylandCompositorsOverlay ];
                    };
              in
              crossPkgs.callPackage services.${n} {
                ps = mkApp {
                  craneLib = crane.mkLib crossPkgs;
                  pkgs = crossPkgs;
                  inherit
                    version
                    advisoryDb
                    ousterSdkSrc
                    ousterSdkNix
                    ;
                  # Shared infra pkgs from exampleAppsPkgsFor are build-time
                  # inputs; use the build host system, not the cross target.
                  exampleAppsPkgs = if exampleAppsPkgsFor != null then exampleAppsPkgsFor host else null;
                };
              };
          }) crossTargets
        ) allServiceNames
      );

      # Non-Linux hosts: short attrs (hello-http) alias the same-CPU Linux cross build.
      crossAliases =
        if lib.hasSuffix "-linux" host then
          { }
        else
          let
            target = defaultLinuxTarget host;
          in
          if target == null then
            { }
          else
            lib.genAttrs allServiceNames (n: crossPackages."${n}-cross-${target}");

      # bundleMembers/defaultBundle are lazy; only referenced for Linux hosts below.
      bundleMembers = lib.map (
        n:
        let
          artifact = appArtifactRawName n host;
        in
        {
          name = artifact;
          path = "${nativeByName.${n}}/${artifact}";
        }
      ) allServiceNames;

      defaultBundle = pkgs.linkFarm "app-image-bundle" bundleMembers;
    in
    nativeByName
    // crossPackages
    // crossAliases
    // lib.optionalAttrs (lib.hasSuffix "-linux" host) {
      default = defaultBundle;
    };

  mkRawCheck =
    pkgs: checkName: serviceName: drv: artifactRaw:
    pkgs.runCommand checkName { } ''
      raw="${drv}/${artifactRaw}"
      test -f "$raw"
      ${pkgs.erofs-utils}/bin/fsck.erofs "$raw" >/dev/null
      ${pkgs.erofs-utils}/bin/dump.erofs --cat --path=etc/os-release "$raw" >os-release
      ${pkgs.erofs-utils}/bin/dump.erofs --path=usr/lib/systemd/system/${serviceName}.service "$raw" >/dev/null
      expect_line() {
        ${pkgs.gnugrep}/bin/grep -qxF "$1" os-release || {
          echo "app image check ${serviceName}: missing os-release line: $1" >&2
          cat os-release >&2
          exit 1
        }
      }
      ${lib.optionalString (serviceName == "hello-ouster-terminal") ''
        ${pkgs.erofs-utils}/bin/dump.erofs --cat --path=usr/lib/jetstream/mission-control/manifest.json "$raw" >manifest.json
        ${pkgs.jq}/bin/jq -e '
          .id == "hello-ouster-terminal" and .version == "${imageVersion}" and
          .nav.label == "Hello Ouster Terminal" and .nav.path == "/app/hello-ouster-terminal" and
          .ui.mode == "external" and .backend.mode == "none" and
          .portable.displayService == true and .portable.gpu == "wayland"
        ' manifest.json >/dev/null
      ''}
      expect_line "ID=${serviceName}"
      expect_line "IMAGE_ID=${serviceName}"
      expect_line 'IMAGE_VERSION="${imageVersion}"'
      expect_line 'VERSION="${imageVersion}"'
      expect_line 'VERSION_ID=1'
      expect_line 'BUILD_ID="${buildId}"'
      touch "$out"
    '';

  perSystemChecks =
    system:
    let
      isLinux = lib.hasSuffix "-linux" system;
      pkgs = appPkgsFor system;
      # Match Teka: macOS delegates app builds to Linux builders.
      hostPkgs = perHostPackages (if isLinux then system else artifactSystem);
      crossTargets = lib.filter (t: t != system) linuxSystems;
      artifactSystem = defaultLinuxTarget system;

      nativeChecks = lib.genAttrs allServiceNames (
        n: mkRawCheck pkgs "app-image-check-${n}" n hostPkgs.${n} (appArtifactRawName n artifactSystem)
      );

      crossChecks = lib.listToAttrs (
        lib.concatMap (
          n:
          map (
            target:
            let
              attr = "${n}-cross-${target}";
              drv = if isLinux then hostPkgs.${attr} else (perHostPackages target).${n};
            in
            {
              name = attr;
              value = mkRawCheck pkgs "app-image-check-${attr}" n drv (appArtifactRawName n target);
            }
          ) crossTargets
        ) allServiceNames
      );

      nativeCargoChecks = lib.foldl' (
        checks: n:
        checks
        // lib.mapAttrs' (checkName: drv: lib.nameValuePair "${n}-${checkName}" drv) (
          hostPkgs.${n}.passthru.cargoChecks or { }
        )
      ) { } allServiceNames;
    in
    lib.optionalAttrs isLinux {
      default = pkgs.runCommand "app-image-check-default" { } ''
        ${lib.concatMapStringsSep "\n" (n: ''
          test -f "${hostPkgs.${n}}/${appArtifactRawName n system}"
        '') allServiceNames}
        touch "$out"
      '';
    }
    // nativeChecks
    // crossChecks
    // nativeCargoChecks;
in
assert services != { };
{
  packages = lib.genAttrs systems perHostPackages;
  checks = lib.genAttrs systems perSystemChecks;
}
