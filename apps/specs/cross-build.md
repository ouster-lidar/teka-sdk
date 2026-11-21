# App image cross-build

App image DDIs target Linux only. Artifacts use two explicit Linux systems:

```text
packages.<buildSystem>.<id>.targets.<targetSystem>
```

Equal systems select a native build; different systems select a cross-build. The short package selectors default to AArch64 when enabled.

## Build an app

Run commands from the repository root. The CLI defaults to an AArch64 target:

```bash
teka app build --arch aarch64 hello-http
```

On an x86_64 Linux host, this selects:

```bash
nix build .#packages.x86_64-linux.hello-http.targets.aarch64-linux
```

## Platform selection

| Host | Target | CLI build system | Build |
|------|--------|-----------------------|-------|
| `x86_64-linux` | `x86_64-linux` | `x86_64-linux` | Native |
| `x86_64-linux` | `aarch64-linux` | `x86_64-linux` | Cross |
| `aarch64-linux` | `aarch64-linux` | `aarch64-linux` | Native |
| `aarch64-linux` | `x86_64-linux` | `aarch64-linux` | Cross |
| `aarch64-darwin` | `aarch64-linux` | `aarch64-linux` | Native on a configured Linux builder |
| `aarch64-darwin` | `x86_64-linux` | `x86_64-linux` | Native on a configured Linux builder |

`teka app build` and `deploy` use the host system on Linux and the target Linux system on macOS. Short Nix selectors on Apple silicon use the AArch64 Linux builder. A successful x86_64-to-ARM cross-build verifies compilation and packaging; it does not execute ARM tests or establish that native ARM builds pass.

App binaries and the runner use target packages and their Crane library. Source preparation and image tools use build-platform packages. TPK packing runs the build-platform CLI.

## Distribution filenames

Each app derivation contains one `$out/<id>-<targetSystem>-v<version>+<buildId>.tpk` file (see [naming.md](naming.md)). `teka app build` links requested apps under `result/<id>` and prints each `.tpk` path. Deploy by name to build for the selected box architectures, or pass a copied `.tpk` path to deploy it to matching boxes.
