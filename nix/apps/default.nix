{
  nixpkgs,
  crane,
  ouster-sdk,
  tekaFor,
}:

{
  self,
  apps,
  systems ? [
    "x86_64-linux"
    "aarch64-linux"
  ],
  version ? import ./version.nix {
    inherit self;
    inherit (nixpkgs) lib;
  },
  advisoryDb ? null,
  ousterSdkSrc ? ouster-sdk,
  tekaSrc ? null,
  overlays ? [ ],
  simpleCrossApps ? [ ],
  imageChecks ? { },
}:
let
  inherit (nixpkgs) lib;
  inherit (version) appArtifactRawName imageVersion buildId;
  appNames = lib.attrNames apps;
  waylandOverlay = import ./overlays/wayland-compositors-overlay.nix;
  crossOverlay = import ./overlays/cross.nix { inherit nixpkgs; };

  packageFor =
    id: buildSystem: targetSystem:
    let
      isCross = buildSystem != targetSystem;
      pkgs = import nixpkgs (
        {
          config.allowUnfree = true;
          overlays =
            overlays
            ++ [ waylandOverlay ]
            ++ lib.optional (isCross && !(lib.elem id simpleCrossApps)) crossOverlay;
        }
        // (
          if isCross then
            {
              localSystem = buildSystem;
              crossSystem = targetSystem;
            }
          else
            { system = buildSystem; }
        )
      );
      ps = pkgs.callPackage ./app-image.nix {
        inherit
          version
          advisoryDb
          ousterSdkSrc
          tekaSrc
          ;
        craneLib = crane.mkLib pkgs;
      };
      image = pkgs.callPackage apps.${id}.image { inherit ps; };
      artifact = appArtifactRawName id targetSystem;
      basename = lib.removeSuffix ".raw" artifact;
      arch = lib.removeSuffix "-linux" targetSystem;
    in
    pkgs.buildPackages.runCommand "${id}-${targetSystem}-tpk"
      {
        nativeBuildInputs = [
          pkgs.buildPackages.jq
          (tekaFor buildSystem)
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

  appPackages = lib.genAttrs appNames (
    id: lib.genAttrs systems (buildSystem: lib.genAttrs systems (packageFor id buildSystem))
  );

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

  checksFor =
    buildSystem:
    let
      pkgs = nixpkgs.legacyPackages.${buildSystem};
    in
    lib.foldl' (
      checks: id:
      checks
      // lib.listToAttrs (
        map (
          targetSystem:
          let
            name = if buildSystem == targetSystem then id else "${id}-cross-${targetSystem}";
            image = appPackages.${id}.${buildSystem}.${targetSystem}.image;
          in
          lib.nameValuePair name (
            mkRawCheck pkgs "app-image-check-${name}" id image (appArtifactRawName id targetSystem)
          )
        ) systems
      )
      // lib.mapAttrs' (name: drv: lib.nameValuePair "${id}-${name}" drv) (
        appPackages.${id}.${buildSystem}.${buildSystem}.image.cargoChecks or { }
      )
    ) { } appNames
    // {
      app-packages = pkgs.runCommand "app-packages-check" { } ''
        ${lib.concatMapStringsSep "\n" (
          id:
          lib.concatMapStringsSep "\n" (
            targetSystem:
            let
              package = appPackages.${id}.${buildSystem}.${targetSystem};
              artifact = lib.removeSuffix ".raw" (appArtifactRawName id targetSystem);
            in
            ''
              test -f ${package}/${artifact}.tpk
            ''
          ) systems
        ) appNames}
        touch "$out"
      '';
    };
in
assert lib.assertMsg (lib.all (
  system:
  lib.elem system [
    "x86_64-linux"
    "aarch64-linux"
  ]
) systems) "Teka app builders and targets must be x86_64-linux or aarch64-linux";
{
  teka.apps = lib.mapAttrs (_: packages: { inherit packages; }) appPackages;
  checks = lib.genAttrs systems checksFor;
}
