# Nix cache

## Configuration

- Configure the publisher's upstream caches in Cachix so paths already available there are not uploaded.

## Publishing

Publish manually alongside GitHub pushes, especially after lockfile updates:

```sh
nix run .#publish-cache
```

- The selected packages, apps, build platforms and destination cache are defined in [`publish-cache.nix`](../nix/publish-cache.nix).
- The command builds the selection, collects locally realized build/runtime dependencies, deduplicates store paths and pushes their manifest closure.
- Nix supplies the tools; authenticate with a cache-scoped `CACHIX_AUTH_TOKEN` or the publisher's Cachix configuration.
- Use `--dry-run` to build and inspect the manifest without uploading or pinning.
- CI/CD integration and scheduled publication are out of scope.

## Release pins

From a clean checkout of the release tag, publish with its name as the pin:

```sh
nix run .#publish-cache -- <release-tag>
```

- The command appends the host system to the pin name and protects the manifest's referenced dependencies from cache eviction.
- Reusing a pin name replaces its previous retained root; different pins share identical store paths.
- Remove obsolete release pins in the Cachix dashboard to allow eviction of paths no longer protected by another pin.
