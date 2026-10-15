# App image erofs DDI format

App images in `apps/` ship as **DDI** `.raw` files. The default format is **erofs** with the same compression tuning as the Jetstream appliance rootfs (see the appliance zedbox-mini-nx image layout spec for OTA benchmarks and kernel codec notes).

At runtime **`systemd-dissect`**, **`portablectl`**, and the host app manager also accept legacy **squashfs** `.raw` images.

## Decision

| Choice | Rationale |
|--------|-----------|
| **erofs** (default) | Better random-access IO than squashfs for Nix store trees; same format as appliance root |
| **lz4hc level 9** | Fast decode on 6.8-tegra; no experimental kernel warnings (unlike deflate) |
| **`-Efragments`** | Packs small tail-end file data into shared fragments (large win on Nix closures) |
| **`-C 16384`** | 16 KiB compression clusters — smaller images than fragments alone; kernel ≥6.6 |
| **`--all-root`** | App images are single-owner trees; matches appliance mkfs |
| **`-T0`** | Reproducible inode timestamps |

**Not used:**

- **deflate-9** — ~3–8% smaller than lz4hc+fragments on the same trees, but **6.8.12-tegra** logs `EROFS: … deflate is experimental` at decode time. Reverted on appliance 2026-06-24.
- **erofs zstd** — needs kernel **6.10+**; product tegra is **6.8.12-tegra**.
- **`-Ededupe`** — ~20 MiB extra savings on a large closure but mkfs ~35× slower (single-threaded in erofs-utils 1.9); deferred.
- **`-Efragments -C 32768` / `65536`** — modest extra savings vs C16k; C16k matches mkosi and appliance default; no product need to chase larger clusters yet.

## Canonical mkfs.erofs

```
mkfs.erofs --all-root -T0 -zlz4hc,level=9 -Efragments -C 16384 <output> <tree>
fsck.erofs <output>
```

**Build wiring (apps flake only):** `nix/erofs-mkfs.nix` emits the shell snippet; `nix/image-builder.nix` → `makeAppImage` is the only erofs entry point. App packages must not call `mkfs.erofs` directly.

The Jetstream appliance rootfs uses the same flag set — keep mkfs flags aligned manually when tuning changes.

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

| App | Target | Flake output | DDI size |
|-----|--------|--------------|---------:|
| `hello-ouster-world` | `x86_64-linux` (native) | `.#hello-ouster-world` | 743,849,984 bytes (709.39 MiB) |
| `hello-ouster-world` | `aarch64-linux` (cross) | `.#hello-ouster-world-cross-aarch64-linux` | 639,840,256 bytes (610.20 MiB) |
| `hello-ouster-dynamo` | `x86_64-linux` (native) | `.#hello-ouster-dynamo` | 743,624,704 bytes (709.18 MiB) |
| `hello-ouster-dynamo` | `aarch64-linux` (cross) | `.#hello-ouster-dynamo-cross-aarch64-linux` | 639,549,440 bytes (609.92 MiB) |

## Coverage

All erofs app images use `ps.makeAppImage`:

| Group | Services |
|-------|----------|
| Examples | [App directories](../examples/) |

## Verify

```bash
# Single mkfs entry point in apps
rg 'mkfs\.erofs' apps/ --glob '!README.md'   # → nix/erofs-mkfs.nix only

cd apps
nix build .#hello-world
nix build .#checks.x86_64-linux.default
```
