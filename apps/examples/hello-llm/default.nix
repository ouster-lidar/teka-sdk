{
  ps,
  pkgs,
  lib,
  stdenv,
  writeText,
  writeShellScript,
  runCommand,
  curl,
  coreutils,
  gnugrep,
}:

let
  app = ps.appCrate;

  appRuntime = pkgs.callPackage ../../nix/app-runtime.nix { };

  name = "hello-llm";
  webPort = 8080;
  ollamaPort = 11434;
  modelTag = "nemotron-3-nano:4b";
  # Bump to force re-pull after changing the default model tag.
  modelStamp = "v1-nemotron-3-nano-4b";

  isAarch64 = stdenv.hostPlatform.system == "aarch64-linux";

  # aarch64: upstream prebuilt (nixpkgs ollama does not cross from x86_64).
  ollamaBackend =
    if isAarch64 then pkgs.callPackage ./ollama-aarch64-prebuilt.nix { } else pkgs.ollama;

  # Container rootfs on all platforms (nixpkgs open-webui pulls torch and is slow to build).
  openWebUi = pkgs.callPackage ./open-webui-docker.nix {
    unpackGnutar = pkgs.buildPackages.gnutar;
    unpackJq = pkgs.buildPackages.jq;
    hostPatchelf = pkgs.buildPackages.patchelf;
    hostWriteShellScript = pkgs.buildPackages.writeShellScript;
  };

  ollamaBin = lib.getExe ollamaBackend;

  jp = ps.hostUnitDefaults {
    hostGpuChecksCfg.enable = false;
  };

  startScript = writeShellScript "${name}-start" ''
    set -euo pipefail
    export PATH="${
      lib.makeBinPath [
        coreutils
        curl
        gnugrep
      ]
    }''${PATH:+:$PATH}"

    state="''${STATE_DIRECTORY:-/var/lib/${name}}"
    home="$state/home"
    models="$state/ollama"
    webui_data="$state/open-webui"
    mkdir -p "$home" "$models" "$webui_data"

    export HOME="$home"
    export OLLAMA_MODELS="$models"
    export OLLAMA_HOST=127.0.0.1:${toString ollamaPort}
    export OLLAMA_MAX_LOADED_MODELS=1
    export OLLAMA_NUM_PARALLEL=1
    export OLLAMA_KEEP_ALIVE=5m

    ${lib.optionalString isAarch64 ''
      # App image /etc has no nv_tegra_release; Ollama skips cuda_jetpack6 without this
      # (discover/runner.go). Host libcuda comes from teka-portable-gpu-generator
      # (X-Teka-PortableGPU=cuda), not BindReadOnlyPaths=/run/opengl-driver.
      export JETSON_JETPACK="''${JETSON_JETPACK:-6}"
      export OLLAMA_LLM_LIBRARY="''${OLLAMA_LLM_LIBRARY:-cuda_jetpack6}"

      cuda_lib="${ollamaBackend}/lib/ollama/cuda_jetpack6"
      if [[ -d "$cuda_lib" ]]; then
        export LD_LIBRARY_PATH="$cuda_lib''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
        echo "${name}: Ollama CUDA (JETSON_JETPACK=$JETSON_JETPACK, OLLAMA_LLM_LIBRARY=$OLLAMA_LLM_LIBRARY)" >&2
      fi
    ''}

    ${ollamaBin} serve &
    ollama_pid=$!
    cleanup() {
      kill "$ollama_pid" 2>/dev/null || true
      wait "$ollama_pid" 2>/dev/null || true
    }
    trap cleanup EXIT INT TERM

    ready=
    for _ in $(seq 1 120); do
      if ${ollamaBin} list >/dev/null 2>&1; then
        ready=1
        break
      fi
      sleep 1
    done
    [[ -n "$ready" ]] || {
      echo "${name}: ollama did not become ready" >&2
      exit 1
    }

    stamp_file="$state/model.stamp"
    need_pull=
    if ! ${ollamaBin} list 2>/dev/null | grep -qF '${modelTag}'; then
      need_pull=1
    elif [[ "$(cat "$stamp_file" 2>/dev/null || true)" != "${modelStamp}" ]]; then
      echo "${name}: re-pulling ${modelTag} (stamp ${modelStamp})" >&2
      ${ollamaBin} rm '${modelTag}' 2>/dev/null || true
      need_pull=1
    fi
    if [[ -n "$need_pull" ]]; then
      echo "${name}: pulling ${modelTag} into $models (first boot)" >&2
      ${ollamaBin} pull '${modelTag}'
      echo "${modelStamp}" >"$stamp_file"
    fi
    echo "${name}: warming ${modelTag}" >&2
    printf '%s\n' 'Say hi in one word.' | ${ollamaBin} run '${modelTag}' >/dev/null || true

    export OLLAMA_BASE_URL="http://127.0.0.1:${toString ollamaPort}"
    export DATA_DIR="$webui_data"
    export WEBUI_SECRET_KEY_FILE="$state/.webui_secret_key"
    export ENV=prod
    # Non-default Open WebUI settings only (v0.9.5 defaults: DDGS_BACKEND=auto, WEB_SEARCH_TRUST_ENV=true, …).
    export ENABLE_WEB_SEARCH=true
    export WEB_SEARCH_ENGINE=duckduckgo
    export BYPASS_WEB_SEARCH_EMBEDDING_AND_RETRIEVAL=true
    export BYPASS_WEB_SEARCH_WEB_LOADER=true

    echo "${name}: Open WebUI on http://0.0.0.0:${toString webPort} (Ollama ${modelTag})" >&2
    ow="${openWebUi}"
    static_rw="$state/open-webui-static"
    mkdir -p "$static_rw"
    if [[ ! -e "$static_rw/favicon.png" ]]; then
      cp -a "$ow/app/backend/open_webui/static/." "$static_rw/"
    fi
    export LANG=C
    unset LC_ALL 2>/dev/null || true
    # Chat-only: skip local embedding/RAG (onnxruntime crashes on Jetson sandbox).
    export BYPASS_EMBEDDING_AND_RETRIEVAL=true
    export RAG_EMBEDDING_MODEL=
    export ENABLE_PERSISTENT_CONFIG=false
    export DATA_DIR=/tmp/hello-llm-state/open-webui
    export WEBUI_SECRET_KEY_FILE=/tmp/hello-llm-state/.webui_secret_key
    # bubblewrap hides DDI /etc/ssl; ddgs/primp verifies TLS (unlike busybox wget).
    bwrap_extra=()
    [[ -r /etc/resolv.conf ]] && bwrap_extra+=(--ro-bind /etc/resolv.conf /etc/resolv.conf)
    [[ -r /etc/hosts ]] && bwrap_extra+=(--ro-bind /etc/hosts /etc/hosts)
    if [[ -r /etc/ssl/certs/ca-certificates.crt ]]; then
      export SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
      bwrap_extra+=(--ro-bind /etc/ssl/certs /etc/ssl/certs)
    fi
    # Container python expects /lib/ld-linux (FHS). Rootfs is read-only; state lives
    # under /tmp/hello-llm-state inside the sandbox (no /var/lib in the image).
    exec ${lib.getExe pkgs.bubblewrap} \
      --ro-bind "$ow" / \
      --bind /sys /sys \
      --proc /proc \
      --dev /dev \
      --tmpfs /tmp \
      --bind "$state" /tmp/hello-llm-state \
      --bind "$static_rw" /app/backend/open_webui/static \
      "''${bwrap_extra[@]}" \
      --setenv PATH /usr/local/bin:/usr/bin:/bin \
      --setenv PYTHONPATH /usr/local/lib/python3.11/site-packages:/app/backend \
      --chdir /app/backend \
      /bin/bash ./start.sh
  '';

  # BindReadOnlyPaths=-/etc/nv_tegra_release needs a file in the read-only DDI (same as
  # resolv.conf stub in makeAppImage); host L4T release overlays at runtime.
  nvTegraReleaseStub =
    if isAarch64 then
      builtins.toFile "${name}-nv-tegra-release-stub" "# placeholder; host bind replaces\n"
    else
      null;

  webTree = app.appWebTree {
    inherit name;
    webSrc = ./web;
    webFiles = [
      "index.html"
      "hello-llm.html"
      "hello-llm.js"
      "hello-llm.css"
    ];
  };

  appManifest = writeText "app-manifest.json" ''
    {
      "id": "hello-llm",
      "version": "${ps.imageVersion}",
      "display_name": "Hello LLM",
      "nav": {
        "label": "Hello LLM",
        "path": "/app/hello-llm",
        "section": "apps",
        "order": 17,
        "panels": [{ "label": "Guide", "panel": "hello-llm" }]
      },
      "ui": {
        "mode": "static",
        "title": "Hello LLM",
        "template": "hello-llm.html",
        "styles": ["hello-llm.css"]
      },
      "backend": { "mode": "none" },
      "auth": {
        "scheme": "ojs-proxy-identity",
        "version": "0.1.0"
      }
    }
  '';

  copyFiles =
    ps.caBundleCopyFiles
    ++ app.appManifestCopyFiles {
      inherit name appManifest webTree;
    }
    ++ lib.optionals (nvTegraReleaseStub != null) [
      {
        path = "etc/nv_tegra_release";
        source = nvTegraReleaseStub;
      }
    ]
    ++ [
      {
        path = "usr/lib/${name}/start.sh";
        source = startScript;
      }
    ];

  runtimeClosures = [
    ollamaBackend
    openWebUi
    appRuntime
    curl
    coreutils
    gnugrep
    pkgs.bash
    pkgs.bubblewrap
  ];
in

ps.makeAppImage {
  inherit name copyFiles runtimeClosures;
  unit = writeText "${name}.service" ''
    [Unit]
    Description=hello-llm (Open WebUI + Ollama/${modelTag})
    ${lib.optionalString isAarch64 "X-Teka-PortableGPU=cuda"}
    After=mission-control.service network-online.target
    Wants=mission-control.service network-online.target

    [Service]
    Type=notify
    PrivateNetwork=no
    ProtectSystem=no
    ProtectHome=no
    StateDirectory=${name}
    StateDirectoryMode=0750
    ${jp.serviceFragment}
    PrivateDevices=no
    SupplementaryGroups=video render
    ExecStart=${appRuntime}/bin/app-runtime-process ${name} tcp ${toString webPort} /usr/lib/${name}/start.sh
    SyslogIdentifier=${name}
    StandardOutput=journal
    StandardError=journal
    BindReadOnlyPaths=-/etc/resolv.conf
    ${lib.optionalString isAarch64 ''
      # DDI stub for bind target (read-only erofs cannot mkdir at runtime).
      BindReadOnlyPaths=-/etc/nv_tegra_release
    ''}
    Restart=on-failure
    RestartSec=5
    TimeoutStartSec=900

    [Install]
    WantedBy=teka-apps.target
  '';
}
