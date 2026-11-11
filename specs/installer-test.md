# Installer smoke test

Run `nix run .#test-build-init` on x86_64 Linux or Apple Silicon macOS. Both runners always stream guest output and Nix build logs, with no argument parsing or verbosity flag. This online test downloads the current Determinate installer and uses Nix caches; run it explicitly rather than as a sandboxed flake check.

Both runners start a fresh guest without Nix, install the portable Teka distribution with `build init`, delete the source executable, and verify installation from a fresh login shell without manually sourcing Nix's profile. They rerun initialization and compile a unique AArch64 Linux hello-world with `build check --full`, including ELF validation.

## Platforms

- [Linux runner](../nix/tests/installer.nix): pinned Ubuntu 24.04 cloud image, disposable qcow2 overlay, NoCloud seed ISO, and separate boot/test serial ports. Runs QEMU as the invoking user with user networking, preferring KVM and falling back to TCG. Installation runs as guest root; no host root, Docker, libvirt, TAP interface, or shared Nix store is required.
- [macOS runner](../nix/tests/installer-darwin.nix): pinned Cirrus Labs macOS 26 Tahoe vanilla image through Tart, with passwordless sudo for the guest administrator. Requires working Apple virtualization, an unlocked login keychain, and SSH routing to the guest; see [Tart prerequisites](https://tart.run/faq/). The first image download is approximately 24 GB; the base stays cached while disposable clones are deleted. Uses unique VM names, random MAC addresses, private known-hosts files, and disables automatic pruning of unrelated images.
- Authenticate macOS guest SSH/SCP through OpenSSH's forced askpass helper, without a controlling terminal or sshpass pseudo-terminal. Keep the private known-hosts file and existing host-key checks.

macOS initialization installs the production VM plugin and registers the system builder. Since the nested guest cannot use hardware virtualization, the runner stops the production monitor and starts the [test-only TCG Linux builder](../nix/tests/builder-tcg.nix). It runs the full check as untrusted `nobody` through the registered daemon builder, verifies daemon trust is false, and leaves trust and signature settings intact. Production uses Apple's Virtualization.framework.

Keep the TCG guest system, kernel, and initrd identical to Nixpkgs' cached builder so preparation requires no running Linux builder. Share private host keys read-only outside the store and configure their mount and SSH bindings at boot, preserving the CLI and daemon's existing host-key pins.

## Results and cleanup

Pass only when guest checks succeed, the success marker is emitted, and the runner exits successfully. The runner files define resource allocations and deadlines for image download, SSH readiness, and guest execution. Disposable disks, clones, and child processes are cleaned up on exit. Interactive confirmation remains a separate case.

Guest output is prefixed with `[macos26]` or `[ubuntu24.04]`; nested labels are adjacent, such as `[macos26][nixos-builder]`. Test output is streamed and saved through `tee`; boot and test logs are separate, retained on failure, and removed on success. Nested builder failures include boot diagnostics and elapsed setup/build time.

Write the TCG console log to an absolute path in the test's working directory so failure cleanup can print it before deleting the guest.

If macOS stalls before guest output begins, inspect the reported IP and SSH error. A VPN may route the guest subnet (usually `192.168.64.0/24`) through a tunnel: check `route -n get <guest-ip>`, then disconnect the VPN or exclude that subnet. If vmnet routing is stale, stop idle VZ/Tart VMs and retry; use `teka build vm stop` for Teka's builder.

## Precedent

The [upstream Nix installer VM tests](https://github.com/NixOS/nix-installer/blob/main/nix/tests/vm-test/default.nix) use pinned cloud images, QEMU overlays, and cloud-init; see their [contributing guide](https://github.com/NixOS/nix-installer/blob/main/CONTRIBUTING.md#testing-installs) and [Cloud-init's QEMU guide](https://docs.cloud-init.io/en/latest/howto/launch_qemu.html).
