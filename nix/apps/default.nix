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
  tekaFor,
  # Services outside this list also receive the PipeWire, onnxruntime,
  # and openblas cross-build workarounds.
  simpleCrossServices ? [ ],
  appOverlays ? [ ],
  imageChecks ? { },
}:
let
  inherit (nixpkgs) lib;
  inherit (version) appArtifactRawName imageVersion buildId;

  waylandCompositorsOverlay = import ./overlays/wayland-compositors-overlay.nix;
  appPkgsFor =
    system:
    nixpkgs.legacyPackages.${system}.appendOverlays (appOverlays ++ [ waylandCompositorsOverlay ]);

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
      # FIXME: Remove when the pin includes https://github.com/NixOS/nixpkgs/pull/577654.
      # Legacy GCC 16 omits the existing libgomp UID buffer fix and fails with -Werror=format-truncation.
      gfortran =
        if prev.stdenv.hostPlatform.system != prev.stdenv.buildPlatform.system then
          prev.wrapCC (
            prev.gfortran.cc.overrideAttrs (old: {
              patches = old.patches ++ [
                (nixpkgs + "/pkgs/development/compilers/gcc/ng/16/libgomp/uid-buffer-size.patch")
              ];
            })
          )
        else
          prev.gfortran;
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
      overlays = appOverlays ++ [ heavyCrossOverlay ];
    };

  mkTpk =
    {
      pkgs,
      image,
      id,
      targetSystem,
    }:
    let
      artifact = appArtifactRawName id targetSystem;
      basename = lib.removeSuffix ".raw" artifact;
      arch = hostCpu targetSystem;
    in
    pkgs.runCommand "${id}-${targetSystem}-tpk"
      {
        nativeBuildInputs = [
          pkgs.jq
          (tekaFor pkgs.stdenv.buildPlatform.system)
        ];
        passthru = { inherit image; };
      }
      ''
        raw=${lib.escapeShellArg "${image}/${artifact}"}
        jq -n \
          --arg arch ${lib.escapeShellArg arch} \
          --arg version ${lib.escapeShellArg version.compose} \
          --arg id ${lib.escapeShellArg id} \
          --arg root_hash "$(cat ${lib.escapeShellArg "${image}/${basename}.roothash"})" \
          '{format:1,arch:$arch,components:[{type:"app",id:$id,version:$version,root_hash:$root_hash,image:{path:($id+".raw")}}]}' >manifest.json
        mkdir -p "$out"
        teka tpk pack --manifest manifest.json \
          --member ${lib.escapeShellArg "${id}.raw"}="$raw" \
          --private-key ${./dev-keys/apps.key} --certificate ${./dev-keys/apps.pem} \
          --output "$out/${basename}.tpk"
      '';

  perHostPackages =
    host:
    let
      builderSystem = defaultLinuxTarget host;
      pkgs = appPkgsFor builderSystem;
      callService = pkgs.callPackage;
      crossTargets = lib.filter (t: t != host) linuxSystems;
      nativeByName =
        if lib.hasSuffix "-linux" host then
          lib.genAttrs allServiceNames (
            n:
            mkTpk {
              inherit pkgs;
              id = n;
              targetSystem = host;
              image = callService services.${n} {
                ps = mkApp {
                  craneLib = crane.mkLib pkgs;
                  inherit
                    pkgs
                    version
                    advisoryDb
                    ousterSdkSrc
                    ousterSdkNix
                    ;
                  exampleAppsPkgs = if exampleAppsPkgsFor != null then exampleAppsPkgsFor builderSystem else null;
                };
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
                    mkCross builderSystem target
                  else
                    import nixpkgs {
                      localSystem = {
                        system = builderSystem;
                      };
                      crossSystem = {
                        system = target;
                      };
                      config.allowUnfree = true;
                      overlays = appOverlays ++ [ waylandCompositorsOverlay ];
                    };
              in
              mkTpk {
                inherit pkgs;
                id = n;
                targetSystem = target;
                image = crossPkgs.callPackage services.${n} {
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
                    exampleAppsPkgs = if exampleAppsPkgsFor != null then exampleAppsPkgsFor builderSystem else null;
                  };
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
          artifact = "${lib.removeSuffix ".raw" (appArtifactRawName n host)}.tpk";
        in
        {
          name = artifact;
          path = "${nativeByName.${n}}/${artifact}";
        }
      ) allServiceNames;

      defaultBundle = pkgs.linkFarm "app-package-bundle" bundleMembers;
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
      root_offset=$(${pkgs.util-linux}/bin/sfdisk --json "$raw" | ${pkgs.jq}/bin/jq -er '.partitiontable | .sectorsize as $sector | .partitions[] | select((.type | ascii_downcase) == "4f68bce3-e8cd-4db1-96e7-fbcaf984b709" or (.type | ascii_downcase) == "b921b045-1df0-41c3-af44-4c6f280d3fae") | .start * $sector')
      ${pkgs.erofs-utils}/bin/fsck.erofs --offset="$root_offset" "$raw" >/dev/null
      ${pkgs.erofs-utils}/bin/dump.erofs --offset="$root_offset" --cat --path=etc/os-release "$raw" >os-release
      ${pkgs.erofs-utils}/bin/dump.erofs --offset="$root_offset" --path=usr/lib/systemd/system/${serviceName}.service "$raw" >/dev/null
      expect_line() {
        ${pkgs.gnugrep}/bin/grep -qxF "$1" os-release || {
          echo "app image check ${serviceName}: missing os-release line: $1" >&2
          cat os-release >&2
          exit 1
        }
      }
      ${(imageChecks.${serviceName} or (_: "")) { inherit pkgs imageVersion; }}
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
        n:
        mkRawCheck pkgs "app-image-check-${n}" n hostPkgs.${n}.image (appArtifactRawName n artifactSystem)
      );

      crossChecks = lib.listToAttrs (
        lib.concatMap (
          n:
          map (
            target:
            let
              attr = "${n}-cross-${target}";
              drv = (if isLinux then hostPkgs.${attr} else (perHostPackages target).${n}).image;
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
          hostPkgs.${n}.image.passthru.cargoChecks or { }
        )
      ) { } allServiceNames;
    in
    lib.optionalAttrs isLinux {
      default = pkgs.runCommand "app-image-check-default" { } ''
        ${lib.concatMapStringsSep "\n" (n: ''
          test -f "${hostPkgs.default}/${lib.removeSuffix ".raw" (appArtifactRawName n system)}.tpk"
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
