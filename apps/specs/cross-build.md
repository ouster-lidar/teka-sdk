# App image cross-build

App image DDIs target **Linux** only. The `apps/` flake `systems` list may include cross-only build hosts (e.g. `aarch64-darwin`) in addition to Linux.

## Short names (`hello-http`)

On **Linux** build hosts, `<id>` is a native build for that system.

On **non-Linux** build hosts (e.g. `aarch64-darwin`), `<id>` is an alias for the same-CPU Linux cross target (`hello-http` → `hello-http-cross-aarch64-linux`).
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

Each app image derivation ships **`$out/<id>-<system>-v<compose>.raw`** (see [naming.md](naming.md)).
The flake **`default`** bundle re-exports every native app image with that basename so mixed-arch upload folders stay unique. **`jetstream-portable-install`** copies into the catalog as **`<id>.raw`** (see [naming.md](naming.md)).

```bash
nix build .#default   # on x86_64-linux: <id>-x86_64-linux-v<compose>.raw for every native app image
```

Cross targets for other CPUs stay explicit (`hello-http-cross-aarch64-linux`, …).

## `systems` summary

| System | `hello-http` | Explicit cross |
|--------|--------------|----------------|
| `x86_64-linux` | native x86_64 | `hello-http-cross-aarch64-linux` |
| `aarch64-linux` | native aarch64 | `hello-http-cross-x86_64-linux` |
| `aarch64-darwin` | cross aarch64-linux | `hello-http-cross-x86_64-linux`, … |

`nix/app-image.nix` derives Linux targets from `systems` via the `*-linux` suffix.

`teka deploy` uses short names when the builder can produce the host system (native Linux or darwin same-CPU alias); otherwise `*-cross-<host-sys>`.
