# Developer Guide

Task-oriented workflow for building app images from `apps/` and deploying them to a Jetstream device.

## Workflows

```mermaid
flowchart TD
    subgraph setup [1. One-time install]
        A[x86_64-linux host]
        B[Install Nix + flakes]
        C[Clone repository]
        K[Install teka]
        A --> B --> C --> K
    end

    subgraph manual [2. Manual build + web UI install]
        D[nix build .#hello-ouster-terminal-cross-aarch64-linux]
        E[result/]
        F[Upload in Jetstream web UI]
        G[Open app and verify]
        D --> E --> F --> G
    end

    subgraph auto [3. teka app deploy auto flow]
        H[Confirm Teka web UI loads]
        I[teka app deploy --build hello-ouster-terminal --host http://jetstream-$SN/]
        J[Open app and verify]
        H --> I --> J
    end

    C --> D
    K --> H
```

The first workflow is required once. Choose either manual web UI installation or the automated deploy command afterward.

## Workflow 1: One-time install

1. Complete the [Linux host setup](linux.md) and [Nix configuration](nix.md).
2. Clone this repository.
3. [Install the Teka CLI](../tools/teka/README.md#install) from the repository root.
4. Change to `apps/`, the directory containing `flake.nix`:

   ```bash
   cd jetstream-example-apps/apps
   ```

5. List available app ids:

   ```bash
   teka app list
   ```

## Workflow 2: Manual build + web UI install

1. From `apps/`, build the app image targeting Jetson `aarch64-linux`:

   ```bash
   nix build .#hello-ouster-terminal-cross-aarch64-linux
   ```

2. In the web UI, upload the image from `result/`.
3. Open the app in the web UI and verify it.

Replace `hello-ouster-terminal` with another app id as needed.

## Workflow 3: `teka app deploy` auto flow

1. Open the Jetstream device web interface in a browser and confirm it loads.
2. Note the Jetstream administrator password set during initial setup.
3. From `apps/`, run:

   ```bash
   export TEKA_PASSWORD='…'
   teka app deploy --build hello-ouster-terminal --host http://zedbox-mini-nx-$SN.local/
   ```

## Verify and iterate

After deploy:

- Open the device app UI or manager for the installed app.
- Read app-specific behavior and checks in each app's README or `*.md` under `apps/examples/`.
- Rerun the same `teka app deploy` command from `apps/` to rebuild and replace an existing install.

## Troubleshooting

| Symptom                                    | See                |
| ------------------------------------------ | ------------------ |
| Device unreachable or WSL networking issue | [wsl2.md](wsl2.md) |
| Flakes disabled, cache misses              | [nix.md](nix.md)   |
