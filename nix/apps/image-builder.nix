{
  name,
  lib,
  stdenv,
  runCommand,
  erofs-utils,
  fakeroot,
  jq,
  systemd,
  util-linux,
  pkgsStatic,
  closureInfo,
  version,
  buildId,
  artifact,
  manifest,
  appDir,
}:
let
  erofs = import ./erofs-mkfs.nix { inherit lib; };
  signedDdi = import ./signed-ddi.nix { inherit lib; };
  # VERSION_ID=1 is the current GPU compatibility generation
  # shared with the host portable-gl-extension (not host or app SemVer).
  defaultOsRelease = builtins.toFile "os-release" ''
    NAME="${name} portable"
    ID=${name}
    ID_LIKE=teka-portable
    IMAGE_ID=${name}
    IMAGE_VERSION="${version}"
    VERSION="${version}"
    VERSION_ID=1
    BUILD_ID="${buildId}"
    PRETTY_NAME="${name} portable ${version}"
    PORTABLE_PREFIXES=${name}
  '';

  buildImage =
    {
      units,
      packages ? [ ],
    }:
    let
      unitDir = "usr/lib/systemd/system";
      servicePath = "${unitDir}/${name}.service";
      serviceLink = "etc/systemd/system/${name}.service";
      manifestPath = "${appDir}/manifest.json";
      stateStubPath = "var/lib/${name}/.keep";
      reservedPaths = [
        "etc/os-release"
        serviceLink
        "nix/store"
      ]
      ++ lib.attrNames files;
      files = {
        ${manifestPath} = manifest;
        ${stateStubPath} = builtins.toFile "${name}-state-stub" "";
      }
      // lib.mapAttrs' (file: lib.nameValuePair "${unitDir}/${file}") units;
      inherit (stdenv.hostPlatform) system;
      busybox = pkgsStatic.busybox;
      architecture = if stdenv.hostPlatform.isAarch64 then "arm64" else "x86-64";
      root = runCommand "${name}-root-tree" { preferLocalBuild = true; } ''
        mkdir -p "$out"
        cd "$out"
        mkdir -p {etc,dev,proc,sys,tmp,var,var/empty,var/tmp,run/dbus,run/systemd/journal,usr/bin,etc/systemd/system,bin} "${unitDir}"
        ln -sfn ../run var/run
        for f in \
          etc/machine-id \
          etc/resolv.conf \
          run/dbus/system_bus_socket \
          run/systemd/journal/socket \
          run/systemd/journal/stdout \
          run/systemd/journal/dev-log; do
          : >"$f"
        done
        chmod -R u+w .
        cp -a ${busybox}/bin/. bin/
        ${lib.optionalString (packages != [ ]) ''
          for package in ${lib.escapeShellArgs (map toString packages)}; do
            cp -aLT --remove-destination -- "$package" .
            find . -type d -exec chmod u+w {} +
          done
          for path in ${lib.escapeShellArgs reservedPaths}; do
            if [ -e "$path" ] || [ -L "$path" ]; then
              echo "Reserved image path from image.packages: $path" >&2
              exit 1
            fi
          done
        ''}
        ${lib.concatStringsSep "\n" (
          lib.mapAttrsToList (path: source: ''
            destination=${lib.escapeShellArg "${path}"}
            mkdir -p "$(dirname -- "$destination")"
            cp -aT --remove-destination -- ${lib.escapeShellArg "${source}"} "$destination"
          '') files
        )}
        # Portable / extension validation: upper `ExtensionImages=` layers must not ship
        # `/usr/lib/os-release` (systemd `extension_has_forbidden_content()`), or dissect
        # returns `-ENOMEDIUM` ("No medium found"). `/etc/os-release` is enough for OS and
        # extension trees — see systemd `extension-util.c` / PORTABLE_SERVICES.md.
        if [ -e usr/lib/os-release ] || [ -L usr/lib/os-release ]; then
          echo "${name}: app images must omit /usr/lib/os-release" >&2
          exit 1
        fi
        install -m644 -T ${defaultOsRelease} etc/os-release
        # Install under usr (per portable services doc); hardlink into etc so dissect
        # discovers the unit on all systemd versions without a duplicate *file* that
        # breaks portablectl detach ("Matching unit files with prefix").
        ln -fT "${servicePath}" "${serviceLink}"
      '';
      # Scan the installed files so copied package outputs do not also ship as store paths.
      closureDrv = closureInfo { rootPaths = [ root ]; };
    in

    assert lib.assertMsg stdenv.buildPlatform.isLinux
      "signed app images require a Linux builder; use teka app build to select one";
    runCommand "${name}-${system}"
      {
        __structuredAttrs = true;
        # Runtime closures are embedded in the signed filesystem image.
        unsafeDiscardReferences.out = true;
        # Avoid shipping large .raw DDIs to/from remote x86 builders when inputs are already local.
        preferLocalBuild = true;
        nativeBuildInputs = [
          erofs-utils
          fakeroot
          jq
          systemd
          util-linux
        ];
        SOURCE_DATE_EPOCH = 0;
        env.SYSTEMD_REPART_MKFS_OPTIONS_EROFS = lib.concatStringsSep " " erofs.args;
      }
      ''
        cp -a ${root} tree
        chmod u+w tree
        mkdir -p tree/nix
        chmod u+w tree/nix
        mkdir -p tree/nix/store
        while IFS= read -r path; do
          [ "$path" = ${lib.escapeShellArg "${root}"} ] && continue
          cp -a "$path" tree/nix/store/
        done < ${closureDrv}/store-paths
        # systemd ships demo units that slow down the host's image scan.
        chmod -R u+w tree/nix/store
        find tree/nix/store -type d -path '*/example/systemd/system' -exec rm -rf {} +

        ${signedDdi {
          inherit architecture;
          image = artifact;
          key = ./dev-keys/apps.key;
          certificate = ./dev-keys/apps.pem;
          rootPartition = ''
            Format=erofs
            Compression=lz4hc
            CompressionLevel=9
            CopyFiles=$PWD/tree:/
            Minimize=best
          '';
        }}
      '';

in
buildImage
