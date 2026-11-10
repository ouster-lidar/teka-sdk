{
  image =
    {
      ps,
      pkgs,
      lib,
      stdenv,
      curl,
      coreutils,
      gnugrep,
      replaceVars,
      replaceVarsWith,
      runtimeShell,
    }:

    let
      app = ps.appCrate;

      inherit (ps) appRuntime;

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

      startScript = replaceVarsWith {
        name = "${name}-start";
        src = ./scripts/start.sh.in;
        isExecutable = true;
        replacements = {
          inherit modelTag modelStamp;
          inherit runtimeShell;
          binPath = lib.makeBinPath [
            coreutils
            curl
            gnugrep
          ];
          ollamaPort = toString ollamaPort;
          isAarch64 = lib.boolToString isAarch64;
          inherit ollamaBackend;
          inherit ollamaBin;
          webPort = toString webPort;
          inherit openWebUi;
          bubblewrap = lib.getExe pkgs.bubblewrap;
        };
      };

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

      appManifest = replaceVars ./manifest.json {
        imageVersion = ps.imageVersion;
      };

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
      unit = replaceVars ./hello-llm.service {
        inherit modelTag;
        gpuUnit = lib.optionalString isAarch64 "X-Teka-PortableGPU=cuda";
        serviceFragment = jp.serviceFragment;
        inherit appRuntime;
        webPort = toString webPort;
        gpuBind = lib.optionalString isAarch64 ''
          # DDI stub for bind target (read-only erofs cannot mkdir at runtime).
          BindReadOnlyPaths=-/etc/nv_tegra_release
        '';
      };
    };
}
