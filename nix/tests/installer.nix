{
  lib,
  fetchurl,
  writeShellApplication,
  qemu,
  cdrkit,
  coreutils,
  gnugrep,
  gnused,
  dist,
  published ? false,
}:
let
  image = fetchurl {
    url = "https://cloud-images.ubuntu.com/releases/noble/release-20261017/ubuntu-24.04-server-cloudimg-amd64.img";
    hash = "sha256:04e8f2461a16a44e78a370284471f29616cdd9baf1893df25807b71f985b6c94";
  };
in
writeShellApplication {
  name = if published then "test-published-install" else "test-build-init";
  runtimeInputs = [
    qemu
    cdrkit
    coreutils
    gnugrep
    gnused
  ];
  text = ''
    ${lib.optionalString published ''
      if (( $# > 1 )) || [[ -n "''${1:-}" && ! "$1" =~ ^v[0-9][A-Za-z0-9._-]*$ ]]; then
        echo 'Usage: test-published-install [RELEASE_TAG]' >&2
        exit 1
      fi
    ''}
    work=$(mktemp -d)
    log=$(mktemp --tmpdir --suffix=.log teka-installer.XXXXXX)
    cleanup() {
      result=$?
      rm -rf "$work"
      if [[ $result == 0 ]]; then
        rm -f "$log" "$log.boot"
      else
        echo "Installer test failed; logs: $log, $log.boot" >&2
      fi
    }
    trap cleanup EXIT
    cd "$work"

    qemu-img create -q -f qcow2 -F qcow2 -b ${image} disk.qcow2
    qemu-img resize -q disk.qcow2 16G
    ${lib.optionalString published ''
      cp ${../../install.sh} install.sh
      printf '%s\n' "''${1:-}" > release-tag
    ''}
    ${lib.optionalString (!published) "cp ${dist}/teka-x86_64-linux teka"}
    echo 'instance-id: teka-installer' > meta-data
    cat > user-data <<EOF
    #cloud-config
    runcmd:
      - mkdir -p /mnt/test
      - mount -o ro /dev/disk/by-label/cidata /mnt/test
      - [su, --login, root, --command, 'bash /mnt/test/test.sh > /dev/ttyS1 2>&1']
    EOF
    cat > test.sh <<'EOF'
    set -euo pipefail
    trap 'poweroff' EXIT
    test ! -e /nix
    test -z "$(command -v nix)"
    ${
      if published then
        ''
          release_tag=$(cat /mnt/test/release-tag)
          if [ -n "$release_tag" ]; then
            cat /mnt/test/install.sh | NIX_INSTALLER_NO_CONFIRM=true sh -s -- "$release_tag"
          else
            curl -fsSL https://raw.githubusercontent.com/ouster-lidar/teka-sdk/main/install.sh | NIX_INSTALLER_NO_CONFIRM=true sh
          fi
        ''
      else
        ''
          install -m755 /mnt/test/teka /teka
          NIX_INSTALLER_NO_CONFIRM=true /teka build init
          rm /teka
        ''
    }
    bash -lec 'set -eu; teka --version; teka build init; teka build check --full --verbose'
    echo TEKA_INSTALLER_TEST_PASSED
    EOF
    genisoimage -quiet -output seed.iso -volid cidata -joliet -rock \
      user-data meta-data test.sh ${if published then "install.sh release-tag" else "teka"}

    echo "Testing installation on Ubuntu 24.04 (log: $log)"
    run_vm() {
      timeout --foreground --kill-after=10s 30m qemu-system-x86_64 \
        -machine accel=kvm:tcg -m 4096 -smp 2 -display none -monitor none \
        -serial file:"$log.boot" -serial stdio \
        -device virtio-rng-pci \
        -drive file=disk.qcow2,format=qcow2,if=virtio \
        -drive file=seed.iso,format=raw,media=cdrom \
        -nic user,model=virtio-net-pci
    }
    run_vm 2>&1 | sed -u 's/^/[ubuntu24.04] /; s/] \[/][/g' | tee "$log"
    grep -qF TEKA_INSTALLER_TEST_PASSED "$log"
    echo "✓ Ubuntu installer test passed."
  '';
}
