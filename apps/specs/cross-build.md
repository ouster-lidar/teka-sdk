# App image cross-build

App image DDIs target Linux only. Artifacts use two explicit Linux systems:

```text
teka.apps.<id>.packages.<buildSystem>.<targetSystem>
```

Equal systems select a native build; different systems select a cross-build. Root `packages` contains SDK and CLI outputs. App artifacts live in `teka.apps` without short names, cross aliases, or a default bundle.

## Build an app

Run commands from the repository root. The CLI defaults to an AArch64 target:

```bash
teka app build --arch aarch64 hello-http
```

On an x86_64 Linux host, this selects:

```bash
nix build .#teka.apps.hello-http.packages.x86_64-linux.aarch64-linux
```

## Platform selection

| Host | Target | Registry build system | Build |
|------|--------|-----------------------|-------|
| `x86_64-linux` | `x86_64-linux` | `x86_64-linux` | Native |
| `x86_64-linux` | `aarch64-linux` | `x86_64-linux` | Cross |
| `aarch64-linux` | `aarch64-linux` | `aarch64-linux` | Native |
| `aarch64-linux` | `x86_64-linux` | `aarch64-linux` | Cross |
| `aarch64-darwin` | `aarch64-linux` | `aarch64-linux` | Native on a configured Linux builder |
| `aarch64-darwin` | `x86_64-linux` | `x86_64-linux` | Native on a configured Linux builder |

`nix/apps/default.nix` exposes only Linux build and target systems. `teka app build` and `deploy` use the host system on Linux and the target Linux system on other hosts. A successful x86_64-to-ARM cross-build verifies compilation and packaging; it does not execute ARM tests or establish that native ARM builds pass.

App binaries and the runner use target packages and their Crane library. Source preparation and image tools use build-platform packages. TPK packing runs the build-platform CLI.

## Distribution filenames

Each app derivation contains one `$out/<id>-<targetSystem>-v<compose>.tpk` file (see [naming.md](naming.md)). `teka app build` links requested apps under `result/<id>`; deployments for multiple architectures use `result/<target>/<id>`.

Image checks retain native names under `checks.<buildSystem>.<id>` and cross names under `checks.<buildSystem>.<id>-cross-<targetSystem>`.
