{
  image =
    {
      ps,
      pkgs,
      lib,
      writeText,
      replaceVars,
      replaceVarsWith,
      runtimeShell,
      ...
    }:

    let
      app = ps.appCrate;

      name = "nvidia-devtools";
      devLibDir = "/usr/lib/nvidia-devtools";
      sshPort = 2223;

      nvidiaDevtools = pkgs.callPackage ./nvidia-devtools.nix { };

      jp = ps.hostUnitDefaults {
        environmentFileWlroots = false;
      };

      nvidiaDevtoolsSrc = app.appRustSrc { srcDir = ./.; };

      utilLinuxMount = pkgs.util-linux.mount;

      nvidiaDevtoolsBin = app.buildAppCrate (
        (app.prepareAppSource {
          crateName = "nvidia-devtools";
          src = nvidiaDevtoolsSrc;
        })
        // {
          meta.mainProgram = "nvidia-devtools";
        }
      );

      devBinPath = lib.makeBinPath [
        nvidiaDevtools
        pkgs.bash
        pkgs.coreutils
      ];

      devPathSh = writeText "${name}-dev-path.sh" ''
        export PATH="${devBinPath}''${PATH:+:$PATH}"
      '';

      devEnv = {
        SSL_CERT_FILE = "/etc/ssl/certs/ca-bundle.crt";
        NIX_SSL_CERT_FILE = "/etc/ssl/certs/ca-bundle.crt";
      };

      devEnvSh = writeText "${name}-dev-env.sh" (
        lib.concatStringsSep "\n" (lib.mapAttrsToList (k: v: "export ${k}=${v}") devEnv)
      );

      devEnvSshdSetEnv = lib.concatStringsSep "\n" (
        lib.mapAttrsToList (k: v: "    SetEnv ${k}=${v}") devEnv
      );

      devBashrc = ./config/bashrc;

      ddiLoginBashProfile = ./config/bash-profile;

      authorizedKeysCommand = replaceVarsWith {
        name = "${name}-authorized-keys";
        src = ./scripts/authorized-keys.sh.in;
        isExecutable = true;
        replacements = {
          inherit runtimeShell;
          coreutils = pkgs.coreutils;
        };
      };

      sshdConfig = replaceVars ./config/sshd_config.in {
        sshPort = toString sshPort;
        openssh = pkgs.openssh;
        inherit devEnvSshdSetEnv;
        inherit devBinPath;
      };

      setupSshd = replaceVarsWith {
        name = "${name}-setup-sshd";
        src = ./scripts/setup-sshd.sh.in;
        isExecutable = true;
        replacements = {
          inherit runtimeShell;
          coreutils = pkgs.coreutils;
          inherit utilLinuxMount;
          openssh = pkgs.openssh;
        };
      };

      sshdRun = pkgs.writeShellScript "${name}-sshd-run" ''
        set -euo pipefail
        dev_root="''${STATE_DIRECTORY:-/var/lib/nvidia-devtools}"
        etc_dir="$dev_root/etc"
        export NSS_WRAPPER_PASSWD="$etc_dir/passwd"
        export NSS_WRAPPER_GROUP="$etc_dir/group"
        export LD_PRELOAD="${pkgs.nss_wrapper}/lib/libnss_wrapper.so"
        exec ${pkgs.openssh}/bin/sshd -D -e -f ${devLibDir}/sshd_config
      '';

      webTree = app.appWebTree {
        inherit name;
        webSrc = ./web;
        webFiles = [
          "nvidia-devtools.html"
          "nvidia-devtools.js"
          "nvidia-devtools.css"
        ];
      };

      appManifest = replaceVars ./manifest.json {
        imageVersion = ps.imageVersion;
      };

      unit = replaceVars ./nvidia-devtools.service {
        sshPort = toString sshPort;
        appBackendUnitLines = ps.appBackendUnitLines;
        appNotifyServiceLines = ps.appNotifyServiceLines;
        inherit devBinPath;
        appBackendServiceLines = ps.appBackendServiceLines;
        serviceFragment = jp.serviceFragment;
        program = lib.getExe nvidiaDevtoolsBin;
      };
    in
    ps.makeAppImage {
      inherit name unit;
      cargoChecks = nvidiaDevtoolsBin.passthru.cargoChecks;
      copyFiles =
        app.appManifestCopyFiles {
          inherit name appManifest webTree;
        }
        ++ ps.caBundleCopyFiles
        ++ lib.mapAttrsToList (path: source: { inherit path source; }) {
          "etc/hosts" = ps.etcHostsStub;
          "etc/nsswitch.conf" = ps.nsswitchConf;
          "etc/passwd" =
            builtins.toFile "${name}-etc-passwd" "root:x:0:0:root:/root:/bin/bash\nsshd:x:999:999:SSH privsep:/var/empty:/bin/sh\njs:x:1000:100:js:/home/js:/bin/bash\n";
          "etc/group" = builtins.toFile "${name}-etc-group" "root:x:0:\nsshd:x:999:\njs:x:100:\n";
          "etc/resolv.conf" = builtins.toFile "${name}-etc-resolv.conf" "nameserver 127.0.0.53\n";
          "root/.bashrc" = devBashrc;
          "root/.bash_profile" = ddiLoginBashProfile;
          "home/js/.bashrc" = devBashrc;
          "home/js/.bash_profile" = ddiLoginBashProfile;
          "etc/profile.d/zz-${name}-binpath.sh" = devPathSh;
          "etc/bash.bashrc" = ./config/bash.bashrc;
          "${devLibDir}/dev-env.sh" = devEnvSh;
          "${devLibDir}/sshd_config" = sshdConfig;
          "${devLibDir}/authorized-keys" = authorizedKeysCommand;
          "${devLibDir}/setup-sshd.sh" = setupSshd;
          "${devLibDir}/sshd-run.sh" = sshdRun;
        };
      runtimeClosures = [
        nvidiaDevtoolsBin
        nvidiaDevtools
        utilLinuxMount
        pkgs.openssh
        pkgs.cacert
        pkgs.coreutils
        pkgs.bash
        pkgs.nss_wrapper
      ];
      rootInteractiveBin = pkgs.symlinkJoin {
        name = "${name}-interactive-bin";
        paths = [
          nvidiaDevtools
          pkgs.bash
          pkgs.coreutils
        ];
      };
    };
}
