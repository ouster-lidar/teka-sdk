{
  lib,
  writeShellApplication,
  writeShellScript,
  fetchurl,
  tart,
  gvproxy,
  python3,
  openssh,
  coreutils,
  gnugrep,
  gnused,
  dist,
  nixpkgsRev,
  published ? false,
}:
let
  # FIXME: Drop this override once Nixpkgs has Tart with --net-fd,
  # which gvproxy needs to avoid VPN-sensitive vmnet routing.
  networkTart = tart.overrideAttrs (
    final: _: {
      version = "2.41.1";
      src = fetchurl {
        url = "https://github.com/openai/tart/releases/download/${final.version}/tart.tar.gz";
        hash = "sha256-5oCnNP68z859TpEMH38T/DsA8ZoYHJReLXRt4yFMMsI=";
      };
    }
  );
in
writeShellApplication {
  name = if published then "test-published-install" else "test-build-init";
  runtimeInputs = [
    networkTart
    gvproxy
    python3
    openssh
    coreutils
    gnugrep
    gnused
  ];
  runtimeEnv = {
    SSH_ASKPASS = writeShellScript "teka-installer-askpass" ''
      printf '%s\n' admin
    '';
    SSH_ASKPASS_REQUIRE = "force";
  };
  text = ''
    ${lib.optionalString published ''
      if (( $# > 1 )) || [[ -n "''${1:-}" && ! "$1" =~ ^v[0-9][A-Za-z0-9._-]*$ ]]; then
        echo 'Usage: test-published-install [RELEASE_TAG]' >&2
        exit 1
      fi
    ''}
    # Keep Unix socket paths below Darwin's 104-byte limit.
    work=$(mktemp -d /tmp/teka-installer.XXXXXX)
    log=$(mktemp --tmpdir --suffix=.log teka-installer.XXXXXX)
    vm="${if published then "teka-published-install" else "teka-build-init"}-$(basename "$work")"
    vm_pid=
    network_pid=
    cleanup() {
      result=$?
      if [[ -n "$vm_pid" ]]; then
        tart stop --timeout 10 "$vm" >> "$log.boot" 2>&1 || true
        kill "$vm_pid" 2>/dev/null || true
        wait "$vm_pid" 2>/dev/null || true
      fi
      if [[ -n "$network_pid" ]]; then
        kill "$network_pid" 2>/dev/null || true
        wait "$network_pid" 2>/dev/null || true
      fi
      tart delete "$vm" >> "$log.boot" 2>&1 || true
      rm -rf "$work"
      if [[ $result == 0 ]]; then
        rm -f "$log" "$log.boot"
      else
        echo "Installer test failed; logs: $log, $log.boot" >&2
      fi
    }
    trap cleanup EXIT
    export TART_NO_AUTO_PRUNE=1

    echo "Testing installation on ARM macOS 26 Tahoe (log: $log)"
    timeout --foreground --kill-after=10s 60m tart clone --concurrency 16 \
      ghcr.io/cirruslabs/macos-tahoe-vanilla@sha256:eeec54bfe1f076e27786c5d92b89187a05b1d109b5071eb2dcdf02d596e34640 \
      "$vm" > "$log.boot" 2>&1
    read -r ssh_port mac < <(python3 - "$work" <<'PY'
    import json
    import secrets
    import socket
    import sys
    mac = "02:" + ":".join(f"{byte:02x}" for byte in secrets.token_bytes(5))
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    with open(f"{sys.argv[1]}/proxy.json", "w") as config:
        json.dump({"stack": {
            "dhcpStaticLeases": {"192.168.127.2": mac},
            "forwards": {f"127.0.0.1:{port}": "192.168.127.2:22"},
        }}, config)
    print(port, mac)
    PY
    )
    tart set "$vm" --cpu 4 --memory 8192 --mac "$mac" >> "$log.boot" 2>&1
    gvproxy -config "$work/proxy.json" -listen-vfkit "unixgram://$work/proxy.sock" >> "$log.boot" 2>&1 &
    network_pid=$!
    timeout --foreground --kill-after=10s 30m \
      python3 ${./tart-network.py} "$work" \
      tart run --no-graphics --no-audio --no-clipboard "$vm" >> "$log.boot" 2>&1 &
    vm_pid=$!

    ssh_options=(
      -F /dev/null
      -o "Port=$ssh_port"
      -o "UserKnownHostsFile=$work/known_hosts"
      -o StrictHostKeyChecking=accept-new
      -o PubkeyAuthentication=no
      -o ConnectTimeout=5
      -o ConnectionAttempts=1
      -o ServerAliveInterval=15
      -o ServerAliveCountMax=2
    )
    guest() {
      timeout --foreground --kill-after=10s "''${ssh_timeout:-25m}" \
        ssh -n "''${ssh_options[@]}" admin@127.0.0.1 "$@"
    }
    deadline=$((SECONDS + 300))
    while true; do
      if ! kill -0 "$vm_pid" "$network_pid" 2>/dev/null; then
        echo "macOS VM or gvproxy exited before SSH became ready" >&2
        tail -n 20 "$log.boot" >&2
        exit 1
      fi
      if ssh_timeout=15s guest true > "$log" 2>&1; then
        break
      fi
      if (( SECONDS >= deadline )); then
        echo "Timed out waiting for macOS guest SSH (127.0.0.1:$ssh_port via gvproxy)" >&2
        cat "$log" >&2
        exit 1
      fi
      sleep 2
    done

    cat > "$work/test.sh" <<'EOF'
    set -euo pipefail
    test "$(uname -s)" = Darwin
    test "$(uname -m)" = arm64
    test ! -e /nix
    test -z "$(command -v nix)"
    sudo -n true
    sudo install -d -o "$(id -u)" -g "$(id -g)" /usr/local/bin
    ${
      if published then
        ''
          release_tag=$(cat /tmp/teka-build-init/release-tag)
          if [ -n "$release_tag" ]; then
            cat /tmp/teka-build-init/install.sh | NIX_INSTALLER_NO_CONFIRM=true sh -s -- "$release_tag"
          else
            curl -fsSL https://raw.githubusercontent.com/ouster-lidar/teka-sdk/main/install.sh | NIX_INSTALLER_NO_CONFIRM=true sh
          fi
        ''
      else
        ''
          chmod 755 /tmp/teka-build-init/teka
          NIX_INSTALLER_NO_CONFIRM=true /tmp/teka-build-init/teka build init
          /tmp/teka-build-init/teka build vm stop
          rm /tmp/teka-build-init/teka
        ''
    }
    # Nix's macOS PATH hook in /etc/zshrc requires an interactive shell.
    /usr/bin/env -i HOME="$HOME" USER="$USER" LOGNAME="$LOGNAME" SHELL=/bin/zsh \
      PATH=/usr/bin:/bin:/usr/sbin:/sbin /bin/zsh -leic '
        set -eu
        teka --version
        teka build init
        # The production monitor needs hardware virtualization; TCG supplies the
        # isolated builder in this nested macOS guest.
        teka build vm stop
        teka build vm status
        started=$SECONDS
        nix build --print-build-logs --impure --file /tmp/teka-build-init/builder-tcg.nix \
          --argstr nixpkgsRev ${nixpkgsRev} --out-link /tmp/teka-build-init/builder
        mkdir /tmp/teka-build-init/linux
        cd /tmp/teka-build-init/linux
        keys="$HOME/.local/share/teka-build-vm/keys"
        KEYS="$keys" /tmp/teka-build-init/builder/bin/teka-test-builder-tcg \
          > boot.log 2>&1 &
        builder_pid=$!
        cleanup_builder() {
          result=$?
          if [[ $result != 0 ]]; then
            tail -n 100 boot.log | sed "s/^/[nixos-builder] /"
            if [[ -f console.log ]]; then tail -n 100 console.log | sed "s/^/[nixos-builder] /"; fi
          fi
          kill -TERM "$builder_pid" 2>/dev/null || true
          wait "$builder_pid" 2>/dev/null || true
        }
        trap cleanup_builder EXIT
        deadline=$((SECONDS + 900))
        until ssh -F /dev/null -o BatchMode=yes -o ConnectTimeout=5 \
          -o StrictHostKeyChecking=yes -o "UserKnownHostsFile=$HOME/.local/share/teka-build-vm/known_hosts" \
          -i "$keys/builder_ed25519" -p 31023 builder@127.0.0.1 true 2>/dev/null; do
          kill -0 "$builder_pid"
          if (( SECONDS >= deadline )); then
            echo "Timed out waiting for the TCG Linux builder" >&2
            exit 1
          fi
          sleep 2
        done
        sudo install -d -o nobody -g nobody -m 700 /tmp/teka-build-user
        untrusted() {
          sudo -u nobody env HOME=/tmp/teka-build-user TMPDIR=/tmp/teka-build-user PATH="$PATH" "$@"
        }
        untrusted nix store info --json | grep -Eq "\"trusted\"[[:space:]]*:[[:space:]]*false"
        untrusted teka build check --full --verbose
        echo "Software-emulated build check passed in $((SECONDS - started)) seconds."
      '
    echo TEKA_INSTALLER_TEST_PASSED
    EOF

    cat > "$work/builder-tcg.nix" <<'EOF'
    ${builtins.readFile ./builder-tcg.nix}
    EOF
    ${lib.optionalString (!published) ''cp ${dist}/teka-aarch64-darwin "$work/teka"''}
    ${lib.optionalString published ''
      cp ${../../install.sh} "$work/install.sh"
      printf '%s\n' "''${1:-}" > "$work/release-tag"
    ''}
    run_test() {
      guest 'mkdir -p /tmp/teka-build-init'
      scp "''${ssh_options[@]}" \
        ${
          if published then ''"$work/install.sh" "$work/release-tag"'' else ''"$work/teka"''
        } "$work/test.sh" "$work/builder-tcg.nix" "admin@127.0.0.1:/tmp/teka-build-init/"
      guest "/bin/zsh -l /tmp/teka-build-init/test.sh"
    }
    run_test 2>&1 | sed -u 's/^/[macos26] /; s/] \[/][/g' | tee "$log"
    grep -qF TEKA_INSTALLER_TEST_PASSED "$log"
    echo "✓ macOS installer test passed."
  '';
}
