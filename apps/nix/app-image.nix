let
  mkApp =
    {
      pkgs,
      craneLib,
      version,
      advisoryDb ? null,
      ousterSdkSrc ? null,
      ousterSdkNix ? ./ouster/ouster-sdk.nix,
      exampleAppsPkgs ? null,
    }:
    let
      inherit (pkgs)
        lib
        writeText
        cacert
        runCommand
        writeShellScript
        ;

      hostGpuChecks = pkgs.callPackage ./wayland/host-gpu-checks.nix { };

      ousterSdk =
        if ousterSdkSrc != null then
          pkgs.callPackage ousterSdkNix {
            src = ousterSdkSrc;
          }
        else
          null;

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

      passwdGroupBindPaths = ''
        BindReadOnlyPaths=-/etc/passwd:/etc/passwd
        BindReadOnlyPaths=-/etc/group:/etc/group
      '';

      # Proxy backends reach the host loopback; readiness is in-process (activate + sd_notify).
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

      # Jetstream appliance conveniences for app unit snippets: optional
      # `EnvironmentFile=/etc/jetstream/wlroots.env` and fail-fast host GPU path
      # checks (see `nix/wayland/host-gpu-checks.nix`). GPU checks are opt-in.
      hostUnitDefaults =
        {
          environmentFileWlroots ? true,
          hostGpuChecksCfg ? { },
        }:
        let
          hc = {
            enable = hostGpuChecksCfg.enable or false;
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

      wayland = import ./wayland/app-support.nix {
        inherit
          lib
          writeText
          version
          caBundleCopyFiles
          etcHostsStub
          nsswitchConf
          hostEtcBindPaths
          passwdGroupBindPaths
          ;
      };

      imageBuilder = mkImageBuilder {
        inherit (pkgs)
          lib
          stdenv
          runCommand
          pkgsStatic
          closureInfo
          ;
        inherit version;
        erofs-utils = pkgs.buildPackages.erofs-utils;
      };
    in
    let
      psBase =
        imageBuilder
        // wayland
        // {
          inherit
            hostGpuChecks
            ousterSdk
            hostUnitDefaults
            etcHostsStub
            nsswitchConf
            caBundlePath
            caBundleCopyFiles
            mkPasswdGroupStubCopyFiles
            mkRestartLoop
            mkAppRunWrapper
            passwdGroupBindPaths
            hostEtcBindPaths
            appBackendUnitLines
            appBackendServiceLines
            appNotifyServiceLines
            ;
        };
      appCrate = import ./app-crate.nix (
        {
          ps = psBase;
          inherit pkgs runCommand craneLib;
          lib = pkgs.lib;
          inherit advisoryDb;
        }
        // lib.optionalAttrs (exampleAppsPkgs != null) {
          appApiJs = exampleAppsPkgs.app-api-js;
          appRuntimeSrc = exampleAppsPkgs.app-runtime-src;
        }
      );
    in
    psBase // {
      inherit appCrate;
      inherit (version) imageVersion;
    };

  mkImageBuilder = import ./image-builder.nix;

  flakeOutputs = import ./flake-outputs.nix { inherit mkApp; };
in
{
  inherit mkApp mkImageBuilder flakeOutputs;
}
