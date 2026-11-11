# App image erofs DDI format

App `.tpk` packages contain signed GPT **DDI** images with erofs and dm-verity per [TPK specification](../../specs/teka-tpk.md).
`makeAppImage` uses `systemd-repart` with the mkfs tuning below.
Bind `roothash` into the package manifest.

## Decision

| Choice | Rationale |
|--------|-----------|
| **erofs** (default) | Random-access IO for Nix store trees |
| **lz4hc level 9** | Fast decode on 6.8-tegra |
| **`-Efragments`** | Packs small tail-end file data into shared fragments (large win on Nix closures) |
| **`-C 16384`** | 16 KiB compression clusters — smaller images than fragments alone; kernel ≥6.6 |
| **`--all-root`** | App images are single-owner trees |
| **`-T0`** | Reproducible inode timestamps |

## Canonical mkfs.erofs

```
mkfs.erofs --all-root -T0 -zlz4hc,level=9 -Efragments -C 16384 <output> <tree>
fsck.erofs <output>
```

**Build wiring:** Importing `nix/apps/erofs-mkfs.nix` with `{ lib, erofs-utils }` returns shared `args` and a `mkfs` function accepting `{ output, sourceDir, extraArgs ? [ ] }`. `nix/apps/image-builder.nix` → `makeAppImage` passes `args` to repart and remains the only erofs entry point. App packages must not call `mkfs.erofs` directly.

## Extension DDI sizes (`x86_64-linux` `*.raw`)

Measured 2026-06-25: **before** = plain `-zlz4hc,level=9` (commit `53e9c36f`); **after** = canonical flags via `erofs-mkfs.nix`.

| Service | Before | After | Δ |
|---------|-------:|------:|--:|
| `hello-world` | 1.04 MiB | 0.97 MiB | −6.8% |
| `hello-http` | 182.95 MiB | 142.55 MiB | −22.1% |
| `hello-wayland-gpu` | 363.88 MiB | 298.16 MiB | −18.1% |
| `hello-firefox` / `hello-firefox-kiosk` | ~850 MiB each | ~715 MiB each | ~−16% |
| `hello-ouster-terminal` | 722.70 MiB | 523.77 MiB | −27.5% |
| `hello-llm` | 3056.68 MiB | 2237.82 MiB | −26.8% |

## Ouster viewer DDI sizes

Measured 2026-09-29 from source revision `2be1287c02f934b298ea3324af8ee6a5cc26e577`. Sizes are exact `.raw` bytes and MiB (1 MiB = 1,048,576 bytes).

| App | Target | DDI size |
|-----|--------|---------:|
| `hello-ouster-world` | `x86_64-linux` (native) | 743,849,984 bytes (709.39 MiB) |
| `hello-ouster-world` | `aarch64-linux` (cross) | 639,840,256 bytes (610.20 MiB) |
| `hello-ouster-dynamo` | `x86_64-linux` (native) | 743,624,704 bytes (709.18 MiB) |
| `hello-ouster-dynamo` | `aarch64-linux` (cross) | 639,549,440 bytes (609.92 MiB) |

## Coverage

All erofs app images use `ps.makeAppImage`:

| Group | Services |
|-------|----------|
| Examples | [App directories](../examples/) |

## Verify

```bash
# Run from the repository root.
rg 'mkfs\.erofs' nix/apps/ --glob '*.nix'
teka app build hello-world
nix build .#checks.x86_64-linux.signed-images
```
