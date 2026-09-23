let
  mkApp =
    pkgs: ousterSdkSrc:
    let
      inherit (pkgs)
        lib
        writeText
        cacert
        runCommand
        writeShellScript
        ;

      hostGpuChecks = pkgs.callPackage ./host-gpu-checks.nix { };

      callAppFirewalld = firewalld: import ./app-firewalld-cmd.nix { inherit lib firewalld; };

      ousterSdk = pkgs.callPackage ./ouster-sdk.nix { src = ousterSdkSrc; };

      callOusterCommon =
        args:
        import ./ouster-app-common.nix (
          {
            inherit lib writeShellScript;
            pickOusterSensorPy = ./pick_ouster_sensor.py;
          }
          // args
        );

      mkFirefoxBin = import ./firefox-bin-app.nix;

      firefoxAppPrefs = writeText "jetstream-portable-firefox-user.js" ''
        user_pref("network.trr.mode", 0);
        user_pref("network.captive-portal-service.enabled", false);
        user_pref("network.connectivity-service.enabled", false);
        user_pref("security.enterprise_roots.enabled", true);
        user_pref("datareporting.policy.dataSubmissionEnabled", false);
        user_pref("toolkit.telemetry.unified", false);
        user_pref("intl.locale.requested", "en-US");
        user_pref("gfx.webrender.all", true);
        user_pref("layers.acceleration.force-enabled", true);
      '';

      firefoxAppProfileSeed = runCommand "jetstream-portable-firefox-profile-seed" { } ''
        mkdir -p "$out"
        install -m444 ${firefoxAppPrefs} "$out/user.js"
      '';

      mkFirefoxClientLoop =
        {
          name,
          firefoxCliTail,
          profileRel,
          nssTools,
          p11-kit,
          firefox-bin,
        }:
        writeShellScript name ''
          set +e
          hostCa=/etc/ssl/certs/ca-bundle.crt
          export SSL_CERT_FILE="${caBundlePath}"
          [[ -r "$hostCa" ]] && export SSL_CERT_FILE="$hostCa"
          export MOZ_ENABLE_WAYLAND=1
          export MOZ_DISABLE_TELEMETRY=1
          export MOZ_LOG="''${MOZ_LOG:-}"
          predir="${firefoxAppProfileSeed}"
          profile="''${HOME}/${profileRel}"
          while true; do
            mkdir -p "$profile"
            install -m444 "$predir/user.js" "$profile/user.js"
            rm -f "$profile/.parentlock" "$profile/lock" 2>/dev/null || true
            if [[ ! -e "$profile/pkcs11.txt" ]]; then
              ${nssTools}/bin/certutil -N --empty-password -d sql:"$profile" >/dev/null 2>&1 || true
            fi
            if ! ${nssTools}/bin/modutil -dbdir sql:"$profile" -list "p11-kit-trust" >/dev/null 2>&1; then
              ${nssTools}/bin/modutil \
                -dbdir sql:"$profile" \
                -force \
                -add "p11-kit-trust" \
                -libfile "${p11-kit}/lib/pkcs11/p11-kit-trust.so" \
                >/dev/null 2>&1 || true
            fi
            ${firefox-bin}/bin/firefox \
              --no-remote \
              --profile "$profile" \
              ${firefoxCliTail} || true
            sleep 1
          done
        '';

      dbusSessionConf = writeText "jetstream-portable-dbus-session.conf" ''
        <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
         "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
          <type>session</type>
          <keep_umask/>
          <listen>unix:tmpdir=/tmp</listen>
          <auth>EXTERNAL</auth>
          <standard_session_servicedirs/>
          <policy context="default">
            <allow send_destination="*" eavesdrop="true"/>
            <allow eavesdrop="true"/>
            <allow own="*"/>
          </policy>
          <includedir>session.d</includedir>
          <includedir>/etc/dbus-1/session.d</includedir>
          <limit name="max_incoming_bytes">1000000000</limit>
          <limit name="max_incoming_unix_fds">250000000</limit>
          <limit name="max_outgoing_bytes">1000000000</limit>
          <limit name="max_outgoing_unix_fds">250000000</limit>
          <limit name="max_message_size">1000000000</limit>
          <limit name="service_start_timeout">120000</limit>
          <limit name="auth_timeout">240000</limit>
          <limit name="pending_fd_timeout">150000</limit>
          <limit name="max_completed_connections">100000</limit>
          <limit name="max_incomplete_connections">10000</limit>
          <limit name="max_connections_per_user">100000</limit>
          <limit name="max_pending_service_starts">10000</limit>
          <limit name="max_names_per_connection">50000</limit>
          <limit name="max_match_rules_per_connection">50000</limit>
          <limit name="max_replies_per_connection">50000</limit>
        </busconfig>
      '';

      etcHostsStub = writeText "jetstream-portable-etc-hosts" "127.0.0.1 localhost\n::1 localhost\n";

      nsswitchConf = writeText "jetstream-portable-nsswitch.conf" ''
        passwd: files
        group: files
        shadow: files
        hosts: files dns
        networks: files
        protocols: files
        services: files
        ethers: files
        rpc: files
        netgroup: files
      '';

      seatdSockStub = builtins.toFile "jetstream-portable-seatd.sock" "";

      hostEtcBindPaths = lib.concatStringsSep "\n" [
        "BindReadOnlyPaths=-/etc/passwd:/etc/passwd"
        "BindReadOnlyPaths=-/etc/group:/etc/group"
        "BindReadOnlyPaths=-/etc/resolv.conf"
        "BindReadOnlyPaths=-/etc/hosts"
        "BindReadOnlyPaths=-/etc/ssl/certs/ca-bundle.crt"
        "BindReadOnlyPaths=-/etc/ssl/certs/ca-certificates.crt"
        "BindReadOnlyPaths=-/etc/pki/tls/certs/ca-bundle.crt"
        "BindReadOnlyPaths=-/run/seatd.sock"
      ];

      wlrootsSwayEnvShSubst =
        {
          clientPath,
          clientLd,
          xdgRuntimeDir,
          bash,
        }:
        let
          tpl = builtins.readFile ./snippets/wlroots-sway-env.sh;
        in
        lib.replaceStrings
          [
            "@clientPath@"
            "@clientLd@"
            "@xdgRuntimeDir@"
            "@bash@"
          ]
          [
            clientPath
            clientLd
            xdgRuntimeDir
            bash
          ]
          tpl;

      caBundlePath = "${cacert}/etc/ssl/certs/ca-bundle.crt";

      caBundleCopyFiles = [
        {
          path = "etc/ssl/certs/ca-bundle.crt";
          source = caBundlePath;
        }
        {
          path = "etc/ssl/certs/ca-certificates.crt";
          source = caBundlePath;
        }
        {
          path = "etc/pki/tls/certs/ca-bundle.crt";
          source = caBundlePath;
        }
      ];

      mkPasswdGroupStubCopyFiles =
        id:
        map
          (pathName: {
            path = "etc/${pathName}";
            source = builtins.toFile "${id}-etc-${pathName}" "# placeholder; service binds host /etc/${pathName} here\n";
          })
          [
            "passwd"
            "group"
          ];

      mkRestartLoop =
        {
          name,
          cmd ? "",
          label ? name,
          fromArgv ? false,
        }:
        writeShellScript name ''
          set +e
          ${lib.optionalString fromArgv ''
            cmd=$1
            label=$2
          ''}
          while true; do
            ${if fromArgv then ''"''$cmd"'' else cmd}
            st=$?
            ${
              if fromArgv then
                ''
                  echo "''${label}: exited with status ''$st (restarting in 1s)" >&2
                ''
              else
                ''
                  echo "${label}: exited with status $st (restarting in 1s)" >&2
                ''
            }
            sleep 1
          done
        '';

      mkAppRunWrapper =
        {
          name,
          innerRun,
          bash,
          extraInstall ? "",
        }:
        runCommand "${name}-run-pkg" { } ''
          mkdir -p "$out"
          install -m755 ${innerRun} "$out/inner.sh"
          ${extraInstall}
          printf '%s\n' '#!/bin/sh' "exec ${lib.getExe bash} $out/inner.sh \"\$@\"" > "$out/run"
          chmod +x "$out/run"
        '';

      mkFontconfigConf =
        {
          name,
          dejavu_fonts,
          cacheDir,
        }:
        writeText name ''
          <?xml version="1.0"?>
          <!DOCTYPE fontconfig SYSTEM "fonts.dtd">
          <fontconfig>
            <dir>${dejavu_fonts}/share/fonts/truetype</dir>
            <cachedir>${cacheDir}</cachedir>
          </fontconfig>
        '';

      waylandAppBaseCopyFiles = [
        {
          path = "etc/dbus-1/session.conf";
          source = dbusSessionConf;
        }
        {
          path = "etc/hosts";
          source = etcHostsStub;
        }
        {
          path = "etc/nsswitch.conf";
          source = nsswitchConf;
        }
      ]
      ++ caBundleCopyFiles
      ++ [
        {
          path = "run/seatd.sock";
          source = seatdSockStub;
        }
      ];

      passwdGroupBindPaths = ''
        BindReadOnlyPaths=-/etc/passwd:/etc/passwd
        BindReadOnlyPaths=-/etc/group:/etc/group
      '';

      # MC proxy backends reach the host loopback; readiness is in-process (activate + sd_notify).
      appBackendUnitLines = ''
        Wants=mission-control.service
      '';

      appNotifyServiceLines = ''
        Type=notify
        NotifyAccess=main
      '';

      appBackendServiceLines = ''
        PrivateNetwork=no
        NetworkNamespacePath=/proc/1/ns/net
      '';

      appGpuManifestJson =
        {
          id,
          gpu,
        }:
        writeText "app-manifest.json" ''
          {"id":"${id}","version":"1.0.0","portable":{"displayService":true,"gpu":"${gpu}"}}
        '';

      appGpuManifestCopyFiles =
        {
          name,
          gpu,
        }:
        [
          {
            path = "usr/lib/jetstream/mission-control/manifest.json";
            source = appGpuManifestJson {
              id = name;
              inherit gpu;
            };
          }
        ];

      mkGpuWaylandUnit =
        {
          name,
          description,
          execStart,
          jp,
          runtimeDirectory,
          useTty ? false,
          bindHostEtc ? true,
          extraUnit ? "",
          extraService ? "",
          user ? "root",
        }:
        writeText "${name}.service" ''
          [Unit]
          Description=${description}
          X-Jetstream-PortableGPU=wayland
          After=systemd-user-sessions.service network.target network-online.target time-sync.target seatd.service
          Wants=network-online.target time-sync.target seatd.service
          Conflicts=getty@tty1.service display-manager.service
          ${extraUnit}

          [Service]
          Type=simple
          User=${user}
          ${jp.serviceFragment}
          ExecStart=${execStart}
          SyslogIdentifier=${name}
          Restart=always
          RestartSec=3
          StandardOutput=journal
          StandardError=journal
          ${lib.optionalString useTty ''
            StandardInput=tty
            TTYPath=/dev/tty1
            TTYReset=true
            TTYVHangup=true
            TTYVTDisallocate=true
          ''}
          ${lib.optionalString bindHostEtc hostEtcBindPaths}
          ${lib.optionalString (!bindHostEtc) passwdGroupBindPaths}
          BindReadOnlyPaths=-/run/seatd.sock
          RuntimeDirectory=${runtimeDirectory}
          RuntimeDirectoryMode=0700
          ${extraService}

          [Install]
          WantedBy=jetstream-apps.target
        '';

      # Jetstream appliance conveniences for portable unit snippets: optional
      # `EnvironmentFile=/etc/jetstream/wlroots.env` and fail-fast host GPU path
      # checks (see `nix/host-gpu-checks.nix`). Booleans use `? true`
      # / `or true` so callers can override individual knobs.
      hostUnitDefaults =
        {
          environmentFileWlroots ? true,
          hostGpuChecksCfg ? { },
        }:
        let
          hc = {
            enable = hostGpuChecksCfg.enable or true;
            vulkanIcds = hostGpuChecksCfg.vulkanIcds or true;
            glvndEglVendors = hostGpuChecksCfg.glvndEglVendors or true;
            seatdSocket = hostGpuChecksCfg.seatdSocket or true;
            gbmBackends = hostGpuChecksCfg.gbmBackends or false;
          };
        in
        {
          serviceFragment = lib.concatStringsSep "\n" (
            lib.optional environmentFileWlroots "EnvironmentFile=/etc/jetstream/wlroots.env"
            ++ [
              "Environment=LANG=C.UTF-8"
              "Environment=LC_ALL=C.UTF-8"
            ]
            ++ lib.optionals hc.enable (
              lib.optional hc.vulkanIcds "ExecStartPre=${hostGpuChecks}/bin/jetstream-portable-check-vulkan-icds"
              ++ lib.optional hc.glvndEglVendors "ExecStartPre=${hostGpuChecks}/bin/jetstream-portable-check-glvnd-egl-vendors"
              ++ lib.optional hc.seatdSocket "ExecStartPre=${hostGpuChecks}/bin/jetstream-portable-check-seatd-socket"
              ++ lib.optional hc.gbmBackends "ExecStartPre=${hostGpuChecks}/bin/jetstream-portable-check-gbm-backends"
            )
          );
          runtimeClosures = lib.optionals hc.enable [ hostGpuChecks ];
        };

      imageBuilder = mkImageBuilder {
        inherit (pkgs)
          lib
          stdenv
          runCommand
          pkgsStatic
          closureInfo
          ;
        erofs-utils = pkgs.buildPackages.erofs-utils;
      };
    in
    imageBuilder
    // {
      inherit
        hostGpuChecks
        callAppFirewalld
        ousterSdk
        callOusterCommon
        hostUnitDefaults
        dbusSessionConf
        etcHostsStub
        nsswitchConf
        seatdSockStub
        caBundlePath
        caBundleCopyFiles
        mkPasswdGroupStubCopyFiles
        mkRestartLoop
        mkAppRunWrapper
        mkFontconfigConf
        waylandAppBaseCopyFiles
        passwdGroupBindPaths
        appGpuManifestCopyFiles
        mkGpuWaylandUnit
        mkFirefoxBin
        mkFirefoxClientLoop
        wlrootsSwayEnvShSubst
        hostEtcBindPaths
        appBackendUnitLines
        appBackendServiceLines
        appNotifyServiceLines
        ;
    };

  mkImageBuilder =
    {
      lib,
      stdenv,
      runCommand,
      erofs-utils,
      pkgsStatic,
      closureInfo,
    }:
    let
      mkCanonicalErofs = import ./erofs-mkfs.nix;
      mkOsRelease =
        name:
        builtins.toFile "os-release" ''
          NAME="${name} portable"
          ID=${name}
          ID_LIKE=jetstream-portable
          VERSION_ID=1
          PORTABLE_PREFIXES=${name}
        '';

      makeAppImage =
        {
          name,
          osRelease ? mkOsRelease name,
          # App image *base* trees (library-only extension with no unit) omit a unit; upper
          # extension images must supply matching `.service` files for the attach prefix.
          unit ? null,
          copyFiles ? [ ],
          runtimeClosures ? [ ],
          # Optional merged `bin/` (full tools via symlinkJoin) copied into the image
          # `tree/bin/` after busybox applets so real binaries override applet names
          # (`lsusb`, `grep`, …). Do not list busybox in `paths` — seeding is below.
          rootInteractiveBin ? null,
        }:
        let
          inherit (stdenv.hostPlatform) system;
          busybox = pkgsStatic.busybox;
          closureDrv = if runtimeClosures == [ ] then null else closureInfo { rootPaths = runtimeClosures; };
          nativeInputs = [ erofs-utils ];
          packImage = mkCanonicalErofs {
            inherit lib erofs-utils;
            output = "$out/${name}.raw";
            sourceDir = "tree";
          };
        in

        runCommand "${name}-${system}"
          {
            nativeBuildInputs = nativeInputs;
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
            # App image / extension validation: upper `ExtensionImages=` layers must not ship
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
              # They are unused in portable DDIs but inflate systemd-dissect --list when MC
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
    };

  flakeOutputs =
    {
      nixpkgs,
      systems,
      services,
      ousterSdkSrc,
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

      allServiceNames = lib.sort lib.lessThan (lib.attrNames services);
      heavyCrossServices = lib.filter (n: !(lib.elem n simpleCrossServices)) allServiceNames;
      # App images target Linux only; other `systems` members (e.g. aarch64-darwin) cross to these.
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
      # nix/pipewire-cross-no-modemmanager-overlay.nix (shared with any separate
      # nixpkgs import that must match this flake's cross package set).
      heavyCrossOverlay =
        final: prev:
        (import ./pipewire-cross-no-modemmanager-overlay.nix) final prev
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
          pkgs = nixpkgs.legacyPackages.${host};
          callService = pkgs.callPackage;
          crossTargets = lib.filter (t: t != host) linuxSystems;
          nativeByName =
            if lib.hasSuffix "-linux" host then
              lib.genAttrs allServiceNames (
                n:
                callService services.${n} {
                  ps = mkApp pkgs ousterSdkSrc;
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
                          overlays = appOverlays;
                        };
                  in
                  crossPkgs.callPackage services.${n} {
                    ps = mkApp crossPkgs ousterSdkSrc;
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
          bundleMembers = lib.map (n: {
            name = "${n}-${host}.raw";
            path = "${nativeByName.${n}}/${n}.raw";
          }) allServiceNames;

          defaultBundle = pkgs.linkFarm "app-image-bundle" bundleMembers;
        in
        nativeByName
        // crossPackages
        // crossAliases
        // lib.optionalAttrs (lib.hasSuffix "-linux" host) {
          default = defaultBundle;
        };

      mkRawCheck =
        pkgs: checkName: serviceName: drv:
        pkgs.runCommand checkName { } ''
          raw=${drv}/${serviceName}.raw
          test -f "$raw"
          if ${pkgs.erofs-utils}/bin/fsck.erofs "$raw" >/dev/null 2>&1; then
            ${pkgs.erofs-utils}/bin/dump.erofs --path=etc/os-release "$raw" >/dev/null
            ${pkgs.erofs-utils}/bin/dump.erofs --path=usr/lib/systemd/system/${serviceName}.service "$raw" >/dev/null
          elif ${pkgs.squashfsTools}/bin/unsquashfs -s "$raw" >/dev/null 2>&1; then
            listing=$(mktemp)
            ${pkgs.squashfsTools}/bin/unsquashfs -l "$raw" >"$listing"
            ${pkgs.gnugrep}/bin/grep -q "etc/os-release" "$listing"
            ${pkgs.gnugrep}/bin/grep -q "usr/lib/systemd/system/${serviceName}.service" "$listing"
            rm -f "$listing"
          else
            echo "portable DDI check ${serviceName}: unsupported image format (want erofs or squashfs): $raw" >&2
            exit 1
          fi
          touch "$out"
        '';

      perSystemChecks =
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          hostPkgs = perHostPackages system;
          crossTargets = lib.filter (t: t != system) linuxSystems;

          nativeChecks = lib.genAttrs allServiceNames (
            n: mkRawCheck pkgs "app-image-check-${n}" n hostPkgs.${n}
          );

          crossChecks = lib.listToAttrs (
            lib.concatMap (
              n:
              map (
                target:
                let
                  attr = "${n}-cross-${target}";
                in
                {
                  name = attr;
                  value = mkRawCheck pkgs "app-image-check-${attr}" n hostPkgs.${attr};
                }
              ) crossTargets
            ) allServiceNames
          );
        in
        lib.optionalAttrs (lib.hasSuffix "-linux" system) {
          default = pkgs.runCommand "app-image-check-default" { } ''
            ${lib.concatMapStringsSep "\n" (n: ''
              test -f ${hostPkgs.${n}}/${n}.raw
            '') allServiceNames}
            touch "$out"
          '';
        }
        // nativeChecks
        // crossChecks;
    in
    assert services != { };
    {
      packages = lib.genAttrs systems perHostPackages;
      checks = lib.genAttrs systems perSystemChecks;
    };
in
{
  inherit mkImageBuilder flakeOutputs;
}
