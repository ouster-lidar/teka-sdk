# apps — `hello-llm` (Open WebUI + Ollama + Nemotron 3 Nano 4B)

Local LLM chat demo: **Open WebUI** on port **8080**, **Ollama** on **127.0.0.1:11434**,
default model **`nemotron-3-nano:4b`** pulled on first boot into persistent state.

| Platform | Open WebUI |
|----------|------------|
| **aarch64** | `ghcr.io/open-webui/open-webui` rootfs (`nix/open-webui-docker.nix`, `arm64`) |
| **x86_64** | Same container rootfs (`nix/open-webui-docker.nix`, `amd64`) |

Image tag and digests live in the Nix expressions, not here. nixpkgs `open-webui` is
avoided on all platforms (pulls torch; aarch64 also lacks rapidocr-onnxruntime).

## Target hardware

**ZED Box Mini** (Jetson Orin NX, JetPack 6). One image for 8 GiB and 16 GiB Orin NX.

## Model

| Item | Choice |
|------|--------|
| Model | [nemotron-3-nano:4b](https://ollama.com/library/nemotron-3-nano) (~2.8 GiB) |
| Storage | `OLLAMA_MODELS` → `/var/lib/hello-llm/ollama` (not in the DDI) |
| First boot | `ollama pull nemotron-3-nano:4b`, then a one-line `ollama run` warm-up |

Bump `modelStamp` in `default.nix` to force a re-pull after changing the default tag.

## Web search

Configured in `hello-llm/default.nix`. Option reference:
[Environment Variable Configuration](https://docs.openwebui.com/reference/env-configuration/),
[DDGS provider](https://docs.openwebui.com/features/chat-conversations/web-search/providers/ddgs/).

| Variable | Value |
|----------|-------|
| `ENABLE_WEB_SEARCH` | `true` |
| `WEB_SEARCH_ENGINE` | `duckduckgo` |
| `BYPASS_WEB_SEARCH_EMBEDDING_AND_RETRIEVAL` | `true` |
| `BYPASS_WEB_SEARCH_WEB_LOADER` | `true` |

## GPU acceleration

| Platform | Ollama | GPU |
|----------|--------|-----|
| **aarch64** | Upstream prebuilt (`nix/ollama-aarch64-prebuilt.nix`) | JetPack 6 `cuda_jetpack6` only |
| **x86_64** | `pkgs.ollama` | CPU (dev smoke) |

## DDI size

The erofs image does **not** embed model weights. Space trims:

- Ollama prebuilt: ship `cuda_jetpack6` only (drop `cuda_v12` / `cuda_v13` from the base tarball)
- Open WebUI rootfs: remove baked `app/backend/data/cache` and `__pycache__` (RAG bypassed at runtime)

## Build

```bash
cd apps
nix build .#hello-llm-cross-aarch64-linux   # from x86_64 → ZED Box Mini
```

## Deploy

```bash
./scripts/jsd-app-deploy.sh hello-llm http://HOST/
```

## App UI

Guide panel at `/app/hello-llm` in Jetstream (how it works, first-boot tips).
Chat UI: `http://<host>:8080/` (TCP 8080 leased through the [SDK runtime](../../lib/app-runtime/runtime-process.md)).
Ollama stays on loopback 11434 without a public lease; Jetstream must use a different port from Open WebUI.
Open WebUI's sandbox receives a read-only snapshot of the service's resolver configuration at launch so host file replacement cannot break startup.

First boot downloads the model (network required; `TimeoutStartSec=900`). Open WebUI data
lives under `/var/lib/hello-llm/open-webui`.

## Verify

```bash
journalctl -u hello-llm -f
curl -sS -o /dev/null -w '%{http_code}\n' http://<host>:8080/
curl -sS http://127.0.0.1:11434/api/tags
```

In Jetstream, open **Apps** → **Hello LLM** → **Logs**.

On Jetson, logs should show `Ollama CUDA (JETSON_JETPACK=6, …)` and inference with GPU layers (`size_vram` > 0 in `/api/ps`). No `/etc/nv_tegra_release` in the DDI — `start.sh` sets `JETSON_JETPACK=6` and `OLLAMA_LLM_LIBRARY=cuda_jetpack6`. The unit uses `X-Jetstream-PortableGPU=cuda` so the host GPU generator binds CUDA store roots without claiming a display connector. Do not `BindReadOnlyPaths=/run/opengl-driver` (symlink breaks namespace setup).

**Nemotron reasoning:** Ollama may emit reasoning traces by default. For CLI smoke tests:

```bash
curl -s http://127.0.0.1:11434/api/generate -d \
  '{"model":"nemotron-3-nano:4b","prompt":"Say hi in one sentence.","stream":false}'
```

In Open WebUI, pick `nemotron-3-nano:4b` from the model list (connected via `OLLAMA_BASE_URL`).
