# App image cross-build

App image DDIs target **Linux** only. The `apps/` flake `systems` list may include cross-only build hosts (e.g. `aarch64-darwin`) in addition to Linux.

## Short names (`hello-http`)

On **Linux** build hosts, `<id>` is a native build for that system.

On **non-Linux** build hosts (e.g. `aarch64-darwin`), `<id>` builds a Linux package for the same CPU.
Explicit cross attributes select other Linux targets.
These builds require a Linux builder.
Use the short name from a Mac:

```bash
nix build .#hello-http
```

On **x86_64-linux** building for an **aarch64** appliance, use the explicit cross attr (short name stays native x86_64):

```bash
nix build .#hello-http-cross-aarch64-linux
```

Explicit `*-cross-<system>` attrs remain on every build host for other targets.

## Distribution filenames

Each public app derivation ships **`$out/<id>-<system>-v<compose>.tpk`** (see [naming.md](naming.md)).
The flake **`default`** bundle re-exports every native app package with that basename so mixed-arch upload folders stay unique.

```bash
nix build .#default   # on x86_64-linux: <id>-x86_64-linux-v<compose>.tpk for every native app package
```

Cross targets for other CPUs stay explicit (`hello-http-cross-aarch64-linux`, …).

## `systems` summary

| System | `hello-http` | Explicit cross |
|--------|--------------|----------------|
| `x86_64-linux` | native x86_64 | `hello-http-cross-aarch64-linux` |
| `aarch64-linux` | native aarch64 | `hello-http-cross-x86_64-linux` |
| `aarch64-darwin` | cross aarch64-linux | `hello-http-cross-x86_64-linux`, … |

`nix/flake-outputs.nix` derives Linux targets from `systems` via the `*-linux` suffix.

`teka app build` / `deploy` never select `packages.<darwin>.*`. A non-Linux builder asks for `packages.<linux-target>.<app>` so a Linux builder produces it. A Linux builder uses the short name for its own system and `<app>-cross-<target>` for the other Linux system.
