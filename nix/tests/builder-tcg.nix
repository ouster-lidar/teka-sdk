{ nixpkgsRev }:
let
  nixpkgs = builtins.getFlake "github:NixOS/nixpkgs/${nixpkgsRev}";
  pkgs = nixpkgs.legacyPackages.aarch64-darwin;
  hostKey = ''
    [Unit]
    Requires=var-hostkeys.mount
    After=var-hostkeys.mount
    [Service]
    BindReadOnlyPaths=/var/hostkeys/ssh_host_ed25519_key:/etc/ssh/ssh_host_ed25519_key
    BindReadOnlyPaths=/var/hostkeys/ssh_host_ed25519_key.pub:/etc/ssh/ssh_host_ed25519_key.pub
  '';
  builder = pkgs.darwin.linux-builder.override {
    modules = [
      ({ lib, ... }: {
        virtualisation.darwin-builder.hostPort = 31023;
        virtualisation.qemu.options = lib.mkAfter [
          # Override Nixpkgs' HVF preference so this test always exercises TCG.
          "-machine accel=tcg"
          "-cpu max"
          "-smp 2"
          "-m 2048"
          "-monitor none"
          "-serial file:\"$CONSOLE_LOG\""
          "-virtfs local,path=\"$HOST_KEYS\",security_model=none,mount_tag=hostkeys,readonly=on"
        ];
      })
    ];
  };
in
assert
  builder.nixosConfig.system.build.toplevel.drvPath
  == pkgs.darwin.linux-builder.nixosConfig.system.build.toplevel.drvPath;
pkgs.writeShellScriptBin "teka-test-builder-tcg" ''
  set -eu
  export HOST_KEYS="$KEYS"
  export CONSOLE_LOG="$PWD/console.log"
  public_keys=$(mktemp -d)
  cp "$KEYS/"*.pub "$public_keys/"
  KEYS="$(nix-store --add "$public_keys")"
  export KEYS
  rm -rf "$public_keys"
  # Runtime credentials preserve the cached guest needed to bootstrap on macOS.
  dropin=$(printf '%s' ${pkgs.lib.escapeShellArg hostKey} | ${pkgs.coreutils}/bin/base64 -w0)
  export QEMU_KERNEL_PARAMS="''${QEMU_KERNEL_PARAMS:-} \
    systemd.mount-extra=hostkeys:/var/hostkeys:9p:ro,trans=virtio,version=9p2000.L,msize=16384 \
    systemd.set_credential_binary=systemd.unit-dropin.sshd.service:$dropin"
  exec ${pkgs.lib.getExe builder.nixosConfig.system.build.vm}
''
