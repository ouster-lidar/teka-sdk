{
  lib,
  stdenv,
  runCommand,
  erofs-utils,
  pkgsStatic,
  closureInfo,
  version,
}:
let
  inherit (version) imageVersion buildId appArtifactRawName;
  mkCanonicalErofs = import ./erofs-mkfs.nix;
  # IMAGE_VERSION / VERSION / BUILD_ID follow the app release scheme. ID stays
  # the portable name. VERSION_ID=1 is the current GPU compatibility generation
  # shared with the host portable-gl-extension (not host or app SemVer).
  mkOsRelease =
    name:
    builtins.toFile "os-release" ''
      NAME="${name} portable"
      ID=${name}
      ID_LIKE=jetstream-portable
      IMAGE_ID=${name}
      IMAGE_VERSION="${imageVersion}"
      VERSION="${imageVersion}"
      VERSION_ID=1
      BUILD_ID="${buildId}"
      PRETTY_NAME="${name} portable ${imageVersion}"
      PORTABLE_PREFIXES=${name}
    '';

  makeAppImage =
    {
      name,
      osRelease ? mkOsRelease name,
      # Portable *base* images (library-only extension with no unit) omit a unit; upper
      # extension images must supply matching `.service` files for the attach prefix.
      unit ? null,
      copyFiles ? [ ],
      runtimeClosures ? [ ],
      cargoChecks ? { },
      # Optional merged `bin/` (full tools via symlinkJoin) copied into the image
      # `tree/bin/` after busybox applets so real binaries override applet names
      # (`lsusb`, `grep`, …). Do not list busybox in `paths` — seeding is below.
      rootInteractiveBin ? null,
    }:
    let
      inherit (stdenv.hostPlatform) system;
      artifactRaw = appArtifactRawName name system;
      busybox = pkgsStatic.busybox;
      closureDrv = if runtimeClosures == [ ] then null else closureInfo { rootPaths = runtimeClosures; };
      packImage = mkCanonicalErofs {
        inherit lib erofs-utils;
        output = "$out/${artifactRaw}";
        sourceDir = "tree";
      };
    in

    runCommand "${name}-${system}"
      {
        # Avoid shipping large .raw DDIs to/from remote x86 builders when inputs are already local.
        preferLocalBuild = true;
        nativeBuildInputs = [ erofs-utils ];
        passthru = { inherit cargoChecks; };
      }
      ''
        mkdir -p tree/{etc,dev,proc,sys,tmp,var,var/empty,var/tmp,run/dbus,run/systemd/journal,usr/lib/systemd/system,usr/bin,etc/systemd/system,bin}
        ln -sfn ../run tree/var/run
        for f in \
          etc/machine-id \
          etc/resolv.conf \
          run/dbus/system_bus_socket \
          run/systemd/journal/socket \
          run/systemd/journal/stdout \
          run/systemd/journal/dev-log; do
          : >"tree/$f"
        done
        chmod -R u+w tree
        cp -a ${busybox}/bin/. tree/bin/
        ${lib.optionalString (rootInteractiveBin != null) ''
          chmod -R u+w tree/bin
          cp -a ${rootInteractiveBin}/bin/. tree/bin/
        ''}
        # Portable / extension validation: upper `ExtensionImages=` layers must not ship
        # `/usr/lib/os-release` (systemd `extension_has_forbidden_content()`), or dissect
        # returns `-ENOMEDIUM` ("No medium found"). `/etc/os-release` is enough for OS and
        # extension trees — see systemd `extension-util.c` / PORTABLE_SERVICES.md.
        cp ${osRelease} tree/etc/os-release
        ${lib.optionalString (unit != null) ''
          # Install under usr (per portable services doc); hardlink into etc so dissect
          # discovers the unit on all systemd versions without a duplicate *file* that
          # breaks portablectl detach ("Matching unit files with prefix").
          cp ${unit} "tree/usr/lib/systemd/system/${name}.service"
          # Same inode as usr copy: some systemd versions only pick up units under
          # etc/systemd/system in the DDI; a second regular copy broke portablectl
          # detach ("Matching unit files with prefix").
          ln "tree/usr/lib/systemd/system/${name}.service" "tree/etc/systemd/system/${name}.service"
        ''}
        ${lib.concatStringsSep "\n" (
          map (f: ''
            mkdir -p "tree/$(dirname ${f.path})"
            cp -a ${f.source} "tree/${f.path}"
          '') copyFiles
        )}
        mkdir -p tree/nix/store
        ${lib.optionalString (closureDrv != null) ''
          while IFS= read -r path; do
            [ -z "$path" ] && continue
            cp -a "$path" tree/nix/store/
          done < ${closureDrv}/store-paths
          # systemd(-minimal) ships hundreds of demo units under example/systemd/system/.
          # They are unused in app images but inflate systemd-dissect --list when host
          # display detection falls through to a unit scan.
          chmod -R u+w tree/nix/store
          find tree/nix/store -type d -path '*/example/systemd/system' -exec rm -rf {} +
        ''}

        mkdir -p "$out"
        ${packImage}
      '';

in
{
  inherit makeAppImage;
}
