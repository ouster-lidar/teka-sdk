{
  nixpkgsRev,
  nixpkgsUnstableRev,
  idleTimeout,
}:
let
  nixpkgs = builtins.getFlake "github:NixOS/nixpkgs/${nixpkgsRev}";
  unstable = builtins.getFlake "github:NixOS/nixpkgs/${nixpkgsUnstableRev}";
  pkgs = nixpkgs.legacyPackages.aarch64-darwin;
  btop = nixpkgs.legacyPackages.aarch64-linux.btop;
  inherit (pkgs) lib;
  shellProfile = pkgs.writeText "teka-build-vm-profile" ''
    export PATH="${lib.makeBinPath [ btop ]}:$PATH"
  '';
  # A local IPv6 address can satisfy network-online before NAT has an IPv4 route.
  # The managed NAT allocator makes client address-conflict probes unnecessary.
  dhcp = "[Service]\nExecStart=\nExecStart=${builder.nixosConfig.systemd.services.dhcpcd.serviceConfig.ExecStart} --waitip=4 --nodelay --noarp\n";
  # Stage the private host key outside the store without rebuilding the cached guest.
  keys = "[Service]\nExecStartPost=${builder.nixosConfig.system.path}/bin/install -m 0600 /var/keys/ssh_host_ed25519_key /run/builder-keys/ssh_host_ed25519_key\nExecStartPost=${builder.nixosConfig.system.path}/bin/ln -sfn ${shellProfile} /etc/profile.local\nExecStartPost=${builder.nixosConfig.systemd.package}/bin/systemd-sysusers --inline \"m builder systemd-journal\"\n";
  hostKey = "[Service]\nBindReadOnlyPaths=/run/builder-keys/ssh_host_ed25519_key:/etc/ssh/ssh_host_ed25519_key\nBindReadOnlyPaths=/run/builder-keys/ssh_host_ed25519_key.pub:/etc/ssh/ssh_host_ed25519_key.pub\n";
  # These settings affect only the host runner, preserving the cached guest closure.
  # Logind can also start tty1 through the autovt alias.
  vzvm = pkgs.writeShellScriptBin "vzvm" ''
    set -eu
    config="$1"
    ${lib.getExe pkgs.jq} \
      --argjson cpus "$TEKA_VM_CPUS" \
      --argjson memory "$TEKA_VM_MEMORY_MIB" \
      --arg dropin ${lib.escapeShellArg "[Socket]\nPollLimitIntervalSec=5s\n"} \
      --arg dhcp ${lib.escapeShellArg dhcp} \
      --arg keys ${lib.escapeShellArg keys} \
      --arg hostKey ${lib.escapeShellArg hostKey} \
      '.cpuCount = $cpus
       | .memorySizeMiB = $memory
       | .cmdline += " systemd.mask=sshd.service systemd.mask=getty@tty1.service systemd.mask=autovt@tty1.service systemd.mask=serial-getty@hvc0.service systemd.setenv=\"NIX_CONFIG=build-dir = /nix/.rw-store/builds\" systemd.set_credential_binary=systemd.unit-dropin.sshd-vsock.socket:\($dropin | @base64) systemd.set_credential_binary=systemd.unit-dropin.dhcpcd.service:\($dhcp | @base64) systemd.set_credential_binary=systemd.unit-dropin.copy-builder-keys.service:\($keys | @base64) systemd.set_credential_binary=systemd.unit-dropin.sshd-vsock@.service:\($hostKey | @base64)"' \
      "$config" > "$config.tmp"
    mv "$config.tmp" "$config"
    exec ${lib.getExe unstable.legacyPackages.aarch64-darwin.vzvm} "$config"
  '';
  builder = pkgs.darwin.linux-builder-vz.override {
    modules = [
      {
        disabledModules = [ "virtualisation/vz-vm.nix" ];
        imports = [ "${unstable}/nixos/modules/virtualisation/vz-vm.nix" ];
        virtualisation.vz = {
          inherit idleTimeout;
          package = vzvm;
          console = "file";
          consoleLog = "console.log";
        };
        virtualisation.darwin-builder.hostPort = 31023;
        virtualisation.additionalPaths = [ shellProfile ];
      }
    ];
  };
in
pkgs.runCommand "teka-build-vm-runtime" { } ''
  mkdir -p $out/bin $out/share/teka-build-vm
  ln -s ${lib.getExe builder.nixosConfig.system.build.vm} $out/bin/teka-build-vm-runner
  echo ${lib.getExe btop} > $out/share/teka-build-vm/btop
''
