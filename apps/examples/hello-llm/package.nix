{
  lib,
  pkgs,
  teka,
}:
let
  name = "hello-llm";
  webPort = "8080";
  modelTag = "nemotron-3-nano:4b";
  # Bump to force re-pull after changing the default model tag.
  modelStamp = "v1-nemotron-3-nano-4b";

  isAarch64 = pkgs.stdenv.hostPlatform.isAarch64;
  # Jetson needs the JetPack build, and Nixpkgs Ollama cannot cross-compile.
  ollamaBackend =
    if isAarch64 then
      pkgs.callPackage ./ollama-aarch64-prebuilt.nix { }
    else if pkgs.stdenv.buildPlatform != pkgs.stdenv.hostPlatform then
      pkgs.callPackage ./ollama-amd64-prebuilt.nix { }
    else
      pkgs.ollama;

  startScript = pkgs.replaceVarsWith {
    name = "${name}-start";
    src = ./scripts/start.sh.in;
    isExecutable = true;
    replacements = {
      inherit (pkgs) runtimeShell;
      inherit
        modelTag
        modelStamp
        ollamaBackend
        webPort
        ;
      ollamaPort = "11434";
      # Use the container rootfs to avoid building PyTorch.
      openWebUi = pkgs.callPackage ./open-webui-docker.nix { };
      binPath = lib.makeBinPath [
        pkgs.coreutils
        pkgs.curl
        pkgs.gnugrep
      ];
      isAarch64 = lib.boolToString isAarch64;
      ollamaBin = lib.getExe ollamaBackend;
      bubblewrap = lib.getExe pkgs.bubblewrap;
    };
  };
in
teka.lib.mkAppImage {
  services.main = {
    runtime.managed = true;
    runtime.network = "host";
    runtime.gpu = if isAarch64 then "cuda" else null;
    systemd = {
      description = "${name} (Open WebUI + Ollama/${modelTag})";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      environment.SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
      serviceConfig = {
        ProtectSystem = false;
        ProtectHome = false;
        StateDirectory = "%N";
        StateDirectoryMode = "0750";
        PrivateDevices = false;
        SupplementaryGroups = "video render";
        ExecStart = "${lib.getExe teka.lib.tekaRunner} ${teka.appId} tcp ${webPort} ${startScript}";
        BindReadOnlyPaths = [ "-/etc/resolv.conf" ] ++ lib.optional isAarch64 "-/etc/nv_tegra_release";
        Restart = "on-failure";
        RestartSec = 5;
        TimeoutStartSec = 900;
      };
    };
  };
  image.packages = [
    (pkgs.linkFarm "${name}-web" { "${teka.appDir}/web" = ./web; })
    pkgs.teka-app-api-js
  ]
  ++ lib.optional isAarch64 (
    # The read-only image needs an existing target for the host L4T release bind.
    pkgs.writeTextDir "etc/nv_tegra_release" "# placeholder; host bind replaces\n"
  );
}
