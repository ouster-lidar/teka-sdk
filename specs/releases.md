# Releases

## Version convention

Release the SDK and CLI together using one version:

| Location | Example |
|----------|---------|
| Git tag and GitHub release title | `v0.1.0` |
| Cargo workspace and Nix package version | `0.1.0` |
| CLI version output | `teka 0.1.0` |
| Pinned Nix reference | `github:ouster-lidar/teka-sdk/v0.1.0#teka` |

The root `Cargo.toml` (`workspace.package.version`) is the source of truth. Both crates inherit it, and the Nix packages read it directly. The `v` prefix belongs only in tags and release titles; [Nix accepts tags as flake references](https://nix.dev/manual/nix/2.28/command-ref/new-cli/nix3-flake.html).

Before 1.0, follow [Cargo's compatibility rules](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html):

- Compatible fixes and features: increment the patch, such as `0.1.0` → `0.1.1`.
- Breaking SDK, CLI, or public Nix interface changes: increment the minor and reset the patch, such as `0.1.4` → `0.2.0`.
- Adopt `1.0.0` when the public interfaces are ready for a stable compatibility commitment. Afterward, use patch for fixes, minor for compatible features, and major for breaking changes.
- Optional release candidates use `0.2.0-rc.1` in Cargo and `v0.2.0-rc.1` as the tag; mark their GitHub releases as prereleases.

Choose and commit the version when preparing a release. Keep it unchanged during ordinary development; do not make a separate post-release bump. Never move a published tag or replace published binaries: corrections get a new version.

## Prepare the release commit

1. Choose the version and update `workspace.package.version` in the root `Cargo.toml`. The first release can retain the existing `0.1.0`.
2. Update the `teka` dependency version in `crates/teka-cli/Cargo.toml` to the same version. Review the independent manifests under `apps/examples/` and update their `teka` requirements as needed, especially across a breaking minor bump. Example app versions remain independent.
3. Refresh the workspace `Cargo.lock` and any affected example lockfiles with Cargo. Review the diff and avoid unrelated dependency upgrades. Verify the resulting workspace with `cargo metadata --locked --no-deps --format-version 1` in the development shell.
4. Commit the version changes with a subject such as `release: Prepare v0.1.1` and a body describing the release's motivation and scope in bullets. Get the release commit onto the release branch before tagging it.

Keep the release's `flake.lock` committed. Updating Nix inputs is a separate choice, not a required part of bumping the version.

## Build and validate

Use a clean checkout of the exact release commit on an Apple Silicon macOS builder and an x86_64 Linux builder. Both builders must use the same commit and lockfiles. These commands work in fish and POSIX shells:

```sh
nix flake check
nix build .#teka.dist --out-link result-release
./result-release/bin/teka --version
```

Confirm the reported version matches the proposed tag without its `v` prefix. `nix flake check` runs checks for the current system, so run it on both platforms. The existing `teka-dist` check validates the portable binary, version, checksums, and platform linkage. Also run `nix run .#test-build-init` on both platforms for an installation smoke test before publishing.

Each builder produces a portable executable and a single-platform `SHA256SUMS`:

- macOS: `result-release/teka-aarch64-darwin`
- Linux: `result-release/teka-x86_64-linux`

Collect both binaries in a fresh directory outside the checkout. Concatenate the two builders' `SHA256SUMS` files into one `SHA256SUMS`; do not overwrite one with the other. From that directory, verify both files with `sha256sum --check SHA256SUMS` (or `shasum -a 256 --check SHA256SUMS` on macOS).

## Tag and publish on GitHub

The commands below use the repository's `github` remote explicitly because `origin` points to GitLab. Run the tag commands from the clean release checkout, substituting the selected version:

```sh
git tag -a v0.1.0 -m 'Teka SDK v0.1.0'
git push github refs/tags/v0.1.0
```

From the directory containing both binaries and the combined checksums, create a draft release using an authenticated GitHub CLI:

```sh
gh release create v0.1.0 \
  --repo ouster-lidar/teka-sdk \
  --verify-tag --draft --title v0.1.0 --generate-notes \
  teka-aarch64-darwin teka-x86_64-linux SHA256SUMS
```

[`--verify-tag` requires the tag to exist on GitHub, and `--draft` leaves the release unpublished](https://cli.github.com/manual/gh_release_create). Review the generated notes, add breaking changes and migration instructions, and confirm all three assets are attached. For release candidates, include `--prerelease` and use the candidate version throughout.

Publish the reviewed draft:

```sh
gh release edit v0.1.0 --repo ouster-lidar/teka-sdk --draft=false
```

Download the published assets and verify their checksums. On each platform, make the downloaded binary executable and check `--version` and `--help`. The stable asset names allow links such as `https://github.com/ouster-lidar/teka-sdk/releases/download/v0.1.0/teka-aarch64-darwin`; users can run `./teka-<system> build init` to install it as described in the [installation guide](../crates/teka-cli/README.md#install).

Once `install.sh` is on GitHub's `main` branch, run `nix run .#test-published-install` on both builders to validate the public installer and latest release in fresh VMs. See the [installer test procedure](installer-test.md).

## Follow-up automation

A future GitHub Actions workflow can trigger on `v*` tags, require the tag to equal `v` plus the Cargo workspace version, build and check both platforms, merge checksums, and upload all assets to a draft release. Keep publication as a review step initially. This repository does not currently have a GitHub release workflow.

Publishing crates to crates.io is a separate release step to decide later; a GitHub release does not publish them. If enabled, publish `teka` before `teka-cli`, which depends on it.
