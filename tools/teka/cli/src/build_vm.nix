{
  nixpkgsRev,
  nixpkgsUnstableRev,
  idleTimeout,
  networkSource,
}:
let
  nixpkgs = builtins.getFlake "github:NixOS/nixpkgs/${nixpkgsRev}";
  unstable = builtins.getFlake "github:NixOS/nixpkgs/${nixpkgsUnstableRev}";
  pkgs = nixpkgs.legacyPackages.aarch64-darwin;
  btop = nixpkgs.legacyPackages.aarch64-linux.btop;
  inherit (pkgs) lib;
  monitor = unstable.legacyPackages.aarch64-darwin.vzvm.overrideAttrs (old: {
    postPatch = (old.postPatch or "") + ''
      cp ${pkgs.writeText "HostNetwork.swift" networkSource} Sources/vzvm/HostNetwork.swift
      substituteInPlace Sources/vzvm/HostNetwork.swift \
        --replace-fail '@gvproxy@' '${lib.getExe' pkgs.gvproxy "gvproxy"}'
      substituteInPlace Sources/vzvm/VirtualMachineBuilder.swift \
        --replace-fail 'network.attachment = VZNATNetworkDeviceAttachment()' \
          'try HostNetwork.shared.configure(network)'
    '';
    doCheck = true;
    checkPhase = ''
      runHook preCheck
      mkdir network-test
      cat > network-test/main.swift <<'EOF'
      import Foundation
      import Virtualization

      // Repeat the ARP exchange to verify gateway access and socket reuse.
      for _ in 0..<2 {
        let network = VZVirtioNetworkDeviceConfiguration()
        try HostNetwork.shared.configure(network)
        let socket = (network.attachment as! VZFileHandleNetworkDeviceAttachment).fileHandle
        let request: [UInt8] = [
          255,255,255,255,255,255, 90,148,239,228,12,238, 8,6,
          0,1, 8,0, 6,4, 0,1, 90,148,239,228,12,238, 192,168,127,2,
          0,0,0,0,0,0, 192,168,127,1
        ]
        try socket.write(contentsOf: Data(request))
        var pending = pollfd(fd: socket.fileDescriptor, events: Int16(POLLIN), revents: 0)
        precondition(poll(&pending, 1, 2000) == 1, "gateway did not reply")
        var response = [UInt8](repeating: 0, count: 2048)
        let count = response.withUnsafeMutableBytes {
          Darwin.read(socket.fileDescriptor, $0.baseAddress, $0.count)
        }
        precondition(count >= 42 && response[20..<22] == [0, 2], "expected an ARP reply")
        precondition(response[28..<32] == [192, 168, 127, 1], "unexpected gateway address")
      }
      EOF
      swiftc -o network-test/check \
        Sources/vzvm/{Config,HostNetwork,Log}.swift network-test/main.swift
      network-test/check
      runHook postCheck
    '';
  });
  shellProfile = pkgs.writeText "teka-build-vm-profile" ''
    export PATH="${lib.makeBinPath [ btop ]}:$PATH"
  '';
  # A local IPv6 address can satisfy network-online before DHCP has an IPv4 route.
  # The managed DHCP allocator makes client address-conflict probes unnecessary.
  dhcp = "[Service]\nExecStart=\nExecStart=${builder.nixosConfig.systemd.services.dhcpcd.serviceConfig.ExecStart} --waitip=4 --nodelay --noarp\n";
  # Stage the private host key outside the store without rebuilding the cached guest.
  keys = "[Service]\nExecStartPost=${builder.nixosConfig.system.path}/bin/install -m 0600 /var/keys/ssh_host_ed25519_key /run/builder-keys/ssh_host_ed25519_key\nExecStartPost=${builder.nixosConfig.system.path}/bin/ln -sfn ${shellProfile} /etc/profile.local\nExecStartPost=${builder.nixosConfig.systemd.package}/bin/systemd-sysusers --inline \"m builder systemd-journal\"\n";
  hostKey = "[Service]\nBindReadOnlyPaths=/run/builder-keys/ssh_host_ed25519_key:/etc/ssh/ssh_host_ed25519_key\nBindReadOnlyPaths=/run/builder-keys/ssh_host_ed25519_key.pub:/etc/ssh/ssh_host_ed25519_key.pub\n";
  # These settings affect only the host runner, preserving the cached guest closure.
  # Logind can also start tty1 through the autovt alias.
  vzvm = pkgs.writeShellScriptBin "vzvm" ''
    set -eu
    config="$1"
    # Normal builds should fit in RAM; swap lets high-concurrency peaks slow down
    # instead of triggering OOM kills. A separate disk preserves build space.
    swap="$(${pkgs.coreutils}/bin/dirname "$config")/swap.img"
    umask 077
    ${pkgs.coreutils}/bin/truncate -s 8G "$swap"
    ${lib.getExe pkgs.jq} \
      --argjson cpus "$TEKA_VM_CPUS" \
      --argjson memory "$TEKA_VM_MEMORY_MIB" \
      --arg dropin ${lib.escapeShellArg "[Socket]\nPollLimitIntervalSec=5s\n"} \
      --arg dhcp ${lib.escapeShellArg dhcp} \
      --arg keys ${lib.escapeShellArg keys} \
      --arg hostKey ${lib.escapeShellArg hostKey} \
      --arg swapDisk "$swap" \
      'if (.disks | length) != 2 then error("expected store and data disks before /dev/vdc swap") else . end
       | .cpuCount = $cpus
       | .memorySizeMiB = $memory
       | .disks += [{path: $swapDisk, readOnly: false}]
       | .cmdline += " systemd.swap-extra=/dev/vdc:x-systemd.makefs systemd.mask=sshd.service systemd.mask=getty@tty1.service systemd.mask=autovt@tty1.service systemd.mask=serial-getty@hvc0.service systemd.setenv=\"NIX_CONFIG=build-dir = /nix/.rw-store/builds\" systemd.set_credential_binary=systemd.unit-dropin.sshd-vsock.socket:\($dropin | @base64) systemd.set_credential_binary=systemd.unit-dropin.dhcpcd.service:\($dhcp | @base64) systemd.set_credential_binary=systemd.unit-dropin.copy-builder-keys.service:\($keys | @base64) systemd.set_credential_binary=systemd.unit-dropin.sshd-vsock@.service:\($hostKey | @base64)"' \
      "$config" > "$config.tmp"
    mv "$config.tmp" "$config"
    exec ${lib.getExe monitor} "$config"
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
