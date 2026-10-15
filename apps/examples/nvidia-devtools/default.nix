{
  ps,
  pkgs,
  lib,
  writeText,
  writeShellScript,
  runCommand,
  ...
}:

let
  app = ps.appCrate;

  name = "nvidia-devtools";
  displayName = "NVIDIA DevTools";
  stateRoot = "/var/lib/nvidia-devtools";
  devLibDir = "/usr/lib/nvidia-devtools";
  sshPort = 2223;

  nvidiaDevtools = pkgs.callPackage ../../nix/nvidia-devtools.nix { };

  jp = ps.hostUnitDefaults {
    environmentFileWlroots = false;
  };

  nvidiaDevtoolsSrc = app.appRustSrc { srcDir = ./.; };

  utilLinuxMount = pkgs.util-linux.mount;

  nvidiaDevtoolsBin = app.buildAppCrate {
    crateName = "nvidia-devtools";
    filteredSrc = nvidiaDevtoolsSrc;
    meta.mainProgram = "nvidia-devtools";
  };

  devShell = "/bin/bash";
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

  devBashrc = writeText "${name}-bashrc" ''
    # shellcheck source=/dev/null
    . /etc/profile.d/zz-${name}-binpath.sh
    . ${devLibDir}/dev-env.sh
    case $- in *i*) ;; *) return ;; esac
    [[ -f /etc/bash.bashrc ]] && . /etc/bash.bashrc
  '';

  ddiEtcBashBashrc = writeText "${name}-etc-bash.bashrc" ''
    PS1='\[\033[38;5;214m\]\u\[\033[38;5;242m\]@\[\033[38;5;80m\]devtools\[\033[0m\] \[\033[38;5;153m\]\W\[\033[0m\] \[\033[38;5;114m\]>\[\033[0m\] '
  '';

  ddiLoginBashProfile = writeText "${name}-login-bash-profile" ''
    # shellcheck source=/dev/null
    [[ -f ~/.bashrc ]] && . ~/.bashrc
  '';

  authorizedKeysCommand = writeShellScript "${name}-authorized-keys" ''
    set -eu
    user="$1"
    case "$user" in
      root) auth="${stateRoot}/ssh/authorized_keys.root" ;;
      js) auth="${stateRoot}/ssh/authorized_keys.js" ;;
      *) exit 0 ;;
    esac
    if [[ ! -r "$auth" ]]; then
      exit 0
    fi
    exec ${pkgs.coreutils}/bin/cat "$auth"
  '';

  sshdConfig = writeText "sshd_config" ''
    Port ${toString sshPort}
    ListenAddress 0.0.0.0
    HostKey ${stateRoot}/ssh/ssh_host_ed25519_key
    AuthorizedKeysFile none
    AuthorizedKeysCommand ${devLibDir}/authorized-keys %u
    AuthorizedKeysCommandUser root
    PermitRootLogin prohibit-password
    PasswordAuthentication no
    KbdInteractiveAuthentication no
    UsePAM no
    StrictModes no
    LogLevel VERBOSE
    Subsystem sftp ${pkgs.openssh}/libexec/sftp-server
    ${devEnvSshdSetEnv}
    SetEnv PATH=${devBinPath}
    SetEnv LD_PRELOAD=
  '';

  setupSshd = writeShellScript "${name}-setup-sshd" ''
    set -euo pipefail
    mkdir=${pkgs.coreutils}/bin/mkdir
    chmod=${pkgs.coreutils}/bin/chmod
    chown=${pkgs.coreutils}/bin/chown
    cp=${pkgs.coreutils}/bin/cp
    mount=${utilLinuxMount}/bin/mount
    dev_root="''${STATE_DIRECTORY:-${stateRoot}}"
    etc_dir="$dev_root/etc"
    dev_shell=${devShell}
    "$mkdir" -p "$dev_root"/{sshd-home,ssh,etc}
    "$chmod" 0755 "$dev_root"
    "$chmod" 0700 "$dev_root/sshd-home" 2>/dev/null || true

    # Empty until a public key is pasted in the app UI. Do not seed a
    # bundled pubkey — that would be a shared login on every deploy.
    init_empty_keys() {
      local file="$1"
      if [[ ! -f "$file" ]]; then
        : >"$file"
        "$chmod" 600 "$file"
      fi
    }

    auth_root="$dev_root/ssh/authorized_keys.root"
    auth_js="$dev_root/ssh/authorized_keys.js"
    init_empty_keys "$auth_root"
    init_empty_keys "$auth_js"

    if [[ ! -f "$dev_root/ssh/ssh_host_ed25519_key" ]]; then
      ${pkgs.openssh}/bin/ssh-keygen -t ed25519 -f "$dev_root/ssh/ssh_host_ed25519_key" -N ""
    fi

    printf '%s\n' \
      "root:x:0:0:root:/root:$dev_shell" \
      'sshd:x:999:999:SSH privsep:/var/empty:/bin/sh' \
      "js:x:1000:100:js:/home/js:$dev_shell" \
      >"$etc_dir/passwd"
    printf '%s\n' 'root:x:0:' 'sshd:x:999:' 'js:x:100:' >"$etc_dir/group"

    resolv="$etc_dir/resolv.conf"
    if [[ -r /run/systemd/resolve/stub-resolv.conf ]]; then
      "$cp" -L /run/systemd/resolve/stub-resolv.conf "$resolv"
    elif [[ -r /etc/resolv.conf ]]; then
      "$cp" -L /etc/resolv.conf "$resolv"
    else
      echo "${name}: no host resolver (stub-resolv.conf or /etc/resolv.conf)" >&2
      exit 1
    fi

    for f in passwd group resolv.conf; do
      [[ -f "$etc_dir/$f" ]] || {
        echo "${name}: missing $etc_dir/$f (StateDirectory not mounted?)" >&2
        exit 1
      }
      "$mount" --bind "$etc_dir/$f" "/etc/$f"
    done
  '';

  sshdRun = writeShellScript "${name}-sshd-run" ''
    set -euo pipefail
    dev_root="''${STATE_DIRECTORY:-${stateRoot}}"
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

  appManifest = writeText "app-manifest.json" ''
    {
      "id": "${name}",
      "version": "${ps.imageVersion}",
      "display_name": "${displayName}",
      "nav": {
        "label": "${displayName}",
        "path": "/app/${name}",
        "section": "apps",
        "order": 26,
        "panels": [{ "label": "${displayName}", "panel": "nvidia-devtools" }]
      },
      "ui": {
        "mode": "static",
        "title": "${displayName}",
        "template": "nvidia-devtools.html",
        "styles": ["nvidia-devtools.css"]
      },
      "backend": {
        "mode": "proxy",
        "runtime_listen": true,
        "health_path": "/api/v1/status"
      },
      "auth": {
        "scheme": "ojs-proxy-identity",
        "version": "0.1.0"
      }
    }
  '';

  devCaps = "CAP_SYS_ADMIN CAP_SYS_CHROOT CAP_NET_RAW CAP_NET_ADMIN CAP_SETUID CAP_SETGID CAP_CHOWN CAP_DAC_OVERRIDE CAP_FOWNER CAP_KILL CAP_SYS_PTRACE";

  unit = writeText "${name}.service" ''
    [Unit]
    Description=${displayName} app (sshd :${toString sshPort})
    X-Jetstream-PortableGPU=cuda
    After=network.target mission-control.service
    Wants=network.target
    ${ps.appBackendUnitLines}

    [Service]
    ${ps.appNotifyServiceLines}
    PrivateNetwork=no
    ProtectSystem=no
    ProtectHome=no
    PrivateDevices=no
    NoNewPrivileges=no
    CapabilityBoundingSet=${devCaps}
    StateDirectory=nvidia-devtools
    StateDirectoryMode=0750
    Environment=APP_ID=${name}
    Environment=PATH=${devBinPath}
    Environment=SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt
    ${ps.appBackendServiceLines}
    ${jp.serviceFragment}
    SupplementaryGroups=video render
    BindReadOnlyPaths=-/run/systemd/resolve:/run/systemd/resolve
    ExecStart=${lib.getExe nvidiaDevtoolsBin}
    SyslogIdentifier=${name}
    StandardOutput=journal
    StandardError=journal
    Restart=on-failure
    KillMode=mixed
    TimeoutStopSec=30

    [Install]
    WantedBy=jetstream-apps.target
  '';
in
ps.makeAppImage {
  inherit name unit;
  copyFiles =
    app.appManifestCopyFiles {
      inherit name appManifest webTree;
    }
    ++ ps.caBundleCopyFiles
    ++ [
      {
        path = "etc/hosts";
        source = ps.etcHostsStub;
      }
      {
        path = "etc/nsswitch.conf";
        source = ps.nsswitchConf;
      }
      {
        path = "etc/passwd";
        source = builtins.toFile "${name}-etc-passwd" "root:x:0:0:root:/root:/bin/bash\nsshd:x:999:999:SSH privsep:/var/empty:/bin/sh\njs:x:1000:100:js:/home/js:/bin/bash\n";
      }
      {
        path = "etc/group";
        source = builtins.toFile "${name}-etc-group" "root:x:0:\nsshd:x:999:\njs:x:100:\n";
      }
      {
        path = "etc/resolv.conf";
        source = builtins.toFile "${name}-etc-resolv.conf" "nameserver 127.0.0.53\n";
      }
      {
        path = "root/.bashrc";
        source = devBashrc;
      }
      {
        path = "root/.bash_profile";
        source = ddiLoginBashProfile;
      }
      {
        path = "home/js/.bashrc";
        source = devBashrc;
      }
      {
        path = "home/js/.bash_profile";
        source = ddiLoginBashProfile;
      }
      {
        path = "etc/profile.d/zz-${name}-binpath.sh";
        source = devPathSh;
      }
      {
        path = "etc/bash.bashrc";
        source = ddiEtcBashBashrc;
      }
      {
        path = "${devLibDir}/dev-env.sh";
        source = devEnvSh;
      }
      {
        path = "${devLibDir}/sshd_config";
        source = sshdConfig;
      }
      {
        path = "${devLibDir}/authorized-keys";
        source = authorizedKeysCommand;
      }
      {
        path = "${devLibDir}/setup-sshd.sh";
        source = setupSshd;
      }
      {
        path = "${devLibDir}/sshd-run.sh";
        source = sshdRun;
      }
    ];
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
}
