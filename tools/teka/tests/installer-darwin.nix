{
  writeShellApplication,
  writeShellScript,
  tart,
  openssh,
  coreutils,
  gnugrep,
  gnused,
  dist,
  nixpkgsRev,
}:
writeShellApplication {
  name = "test-build-init";
  runtimeInputs = [
    tart
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
    work=$(mktemp -d)
    log=$(mktemp --tmpdir --suffix=.log teka-installer.XXXXXX)
    vm="teka-build-init-$(basename "$work")"
    vm_pid=
    cleanup() {
      result=$?
      if [[ -n "$vm_pid" ]]; then
        tart stop --timeout 10 "$vm" >> "$log.boot" 2>&1 || true
        kill "$vm_pid" 2>/dev/null || true
        wait "$vm_pid" 2>/dev/null || true
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
    tart set "$vm" --cpu 4 --memory 8192 --random-mac >> "$log.boot" 2>&1
    timeout --foreground --kill-after=10s 30m \
      tart run --no-graphics --no-audio --no-clipboard "$vm" >> "$log.boot" 2>&1 &
    vm_pid=$!

    ssh_options=(
      -F /dev/null
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
        ssh -n "''${ssh_options[@]}" "admin@$ip" "$@"
    }
    deadline=$((SECONDS + 300))
    while true; do
      if ! kill -0 "$vm_pid" 2>/dev/null; then
        echo "macOS VM exited before SSH became ready" >&2
        exit 1
      fi
      if ip=$(tart ip "$vm" 2> "$log") && ssh_timeout=15s guest true > "$log" 2>&1; then
        break
      fi
      if (( SECONDS >= deadline )); then
        echo "Timed out waiting for macOS guest SSH (''${ip:-no IP address})" >&2
        cat "$log" >&2
        echo "A VPN may be routing the guest subnet through its tunnel. Disconnect it or exclude the guest subnet (usually 192.168.64.0/24), then retry." >&2
        echo "If vmnet routing is stale, stop idle VZ/Tart VMs and retry." >&2
        exit 1
      fi
      sleep 2
    done

    cat > "$work/test.sh" <<'EOF'
    set -eu
    test "$(uname -s)" = Darwin
    test "$(uname -m)" = arm64
    test ! -e /nix
    test -z "$(command -v nix)"
    sudo -n true
    sudo install -d -o "$(id -u)" -g "$(id -g)" /usr/local/bin
    chmod 755 /tmp/teka-build-init/teka
    NIX_INSTALLER_NO_CONFIRM=true /tmp/teka-build-init/teka build init
    /tmp/teka-build-init/teka build vm stop
    rm /tmp/teka-build-init/teka
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
    cp ${dist}/teka-aarch64-darwin "$work/teka"
    run_test() {
      guest 'mkdir -p /tmp/teka-build-init'
      scp "''${ssh_options[@]}" \
        "$work/teka" "$work/test.sh" "$work/builder-tcg.nix" "admin@$ip:/tmp/teka-build-init/"
      guest "/bin/zsh -l /tmp/teka-build-init/test.sh"
    }
    run_test 2>&1 | sed -u 's/^/[macos26] /; s/] \[/][/g' | tee "$log"
    grep -qF TEKA_INSTALLER_TEST_PASSED "$log"
    echo "✓ macOS installer test passed."
  '';
}
