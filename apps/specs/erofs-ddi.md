# Portable app erofs DDI format

Portable images in `apps/` ship as **DDI** `.raw` files. The default format is
**erofs** with lz4hc level 9, fragments, and 16 KiB clusters.

Runtime (`systemd-dissect`, `portablectl`, Jetstream) accepts erofs (and
squashfs, unused by these demos).

## Decision

| Choice | Rationale |
|--------|-----------|
| **erofs** (default) | Better random-access IO than squashfs for Nix store trees |
| **lz4hc level 9** | Fast decode on current Jetson kernels |
| **`-Efragments`** | Packs tail-end file fragments to reduce wasted space |
| **`-C 16384`** | 16 KiB compression clusters; kernel ≥6.6 |
| **`--all-root`** | Portable images are single-owner trees |
| **`-T0`** | Reproducible inode timestamps |

## Canonical mkfs.erofs

```
mkfs.erofs --all-root -T0 -zlz4hc,level=9 -Efragments -C 16384 <output> <tree>
fsck.erofs <output>
```

**Build wiring:** `nix/erofs-mkfs.nix` emits the shell snippet;
`nix/portable-service.nix` → `makePortableErofsImage` is the only erofs entry
point. App packages must not call `mkfs.erofs` directly.

## Verify

```bash
cd apps
nix build .#hello-world
nix build .#checks.x86_64-linux.hello-world
```
