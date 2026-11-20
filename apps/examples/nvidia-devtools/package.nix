{ teka }:
teka.lib.mkAppImage (
  { lib, pkgs, ... }:
  let
    inherit (pkgs)
      writeText
      replaceVars
      replaceVarsWith
      runtimeShell
      ;

    name = "nvidia-devtools";
    devLibDir = "usr/lib/nvidia-devtools";
    sshPort = "2223";

    nvidiaDevtools = pkgs.callPackage ./nvidia-devtools.nix { };

    utilLinuxMount = pkgs.util-linux.mount;

    nvidiaDevtoolsBin = teka.lib.buildRustApp {
      srcDir = ./.;
    };

    shellTools = [
      pkgs.bash
      pkgs.coreutils
    ];
    devBinPath = lib.makeBinPath ([ nvidiaDevtools ] ++ shellTools);

    # Nsight tools locate their data relative to the installed executable.
    toolLaunchers =
      pkgs.runCommand "${name}-tool-launchers"
        {
          nativeBuildInputs = [ pkgs.buildPackages.makeWrapper ];
        }
        ''
          mkdir -p "$out/bin"
          for tool in ${nvidiaDevtools}/bin/*; do
            if [[ -f "$tool" && -x "$tool" ]]; then
              makeWrapper "$tool" "$out/bin/''${tool##*/}"
            fi
          done
        '';

    devPathSh = writeText "${name}-dev-path.sh" ''
      export PATH="${devBinPath}''${PATH:+:$PATH}"
    '';

    devEnv = rec {
      SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
      NIX_SSL_CERT_FILE = SSL_CERT_FILE;
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
      inherit sshPort;
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
      exec ${pkgs.openssh}/bin/sshd -D -e -f /${devLibDir}/sshd_config
    '';
  in
  {
    services.main = {
      program = nvidiaDevtoolsBin;
      runtime.managed = true;
      runtime.network = "host";
      runtime.gpu = "cuda";
      systemd = {
        description = "NVIDIA DevTools app (sshd :${sshPort})";
        after = [ "network.target" ];
        wants = [ "network.target" ];
        environment = {
          PATH = devBinPath;
          inherit (devEnv) SSL_CERT_FILE;
        };
        serviceConfig = {
          ProtectSystem = false;
          ProtectHome = false;
          PrivateDevices = false;
          NoNewPrivileges = false;
          CapabilityBoundingSet = "CAP_SYS_ADMIN CAP_SYS_CHROOT CAP_NET_RAW CAP_NET_ADMIN CAP_SETUID CAP_SETGID CAP_CHOWN CAP_DAC_OVERRIDE CAP_FOWNER CAP_KILL CAP_SYS_PTRACE";
          StateDirectory = name;
          StateDirectoryMode = "0750";
          SupplementaryGroups = "video render";
          BindReadOnlyPaths = "-/run/systemd/resolve:/run/systemd/resolve";
          Restart = "on-failure";
          KillMode = "mixed";
          TimeoutStopSec = 30;
        };
      };
    };
    image = {
      packages = [
        (pkgs.linkFarm "${name}-web" { "${teka.appDir}/web" = ./web; })
        teka.lib.webApi

        pkgs.dockerTools.caCertificates
        pkgs.fakeNss
        (pkgs.linkFarm "${name}-root" {
          "etc/hosts" = writeText "teka-portable-etc-hosts" "127.0.0.1 localhost\n::1 localhost\n";
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
        })
        (pkgs.buildEnv {
          name = "${name}-shell-tools";
          paths = [ toolLaunchers ] ++ map lib.getBin shellTools;
          pathsToLink = [ "/bin" ];
        })
      ];
    };
  }
)
