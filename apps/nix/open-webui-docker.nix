# Open WebUI from the official multi-arch container rootfs.
# nixpkgs open-webui pulls torch/transformers and is slow to build; aarch64 does not
# evaluate cleanly (rapidocr-onnxruntime). Unpack ghcr.io instead.
{
  lib,
  stdenv,
  buildPackages,
  unpackGnutar,
  unpackJq,
  hostPatchelf,
  hostWriteShellScript,
  dockerArch ? if stdenv.hostPlatform.system == "aarch64-linux" then "arm64" else "amd64",
}:

let
  version = "0.9.5";
  imageTag = "v${version}";

  imageConfigs = {
    arm64 = {
      imageDigest = "sha256:e045bde3b004cc7f8c319412345eb56c87ea6ac57031534a31ca37ad5424beb3";
      imageTarHash = "sha256-wGSqSCui2TJE96yjwzoLzQM+1f0Vu6TjBcbYeRgNNWQ=";
    };
    amd64 = {
      imageDigest = "sha256:ef3eaeb6235dd86d8ae7425e6af38272b0cea27896c0d5ae8c5cf23f886de76a";
      imageTarHash = "sha256-kaFQ3QIIGiL4vp3aDLnxzg/i+ipiBoPib4R16/RkoZ8=";
    };
  };

  cfg =
    imageConfigs.${dockerArch} or (throw "open-webui-docker: unsupported dockerArch ${dockerArch}");

  # Pull on the build platform; preferLocalBuild avoids remote builders that cannot
  # bind /etc/resolv.conf into the docker pull chroot.
  image =
    (buildPackages.dockerTools.pullImage {
      imageName = "ghcr.io/open-webui/open-webui";
      imageDigest = cfg.imageDigest;
      hash = cfg.imageTarHash;
      arch = dockerArch;
      finalImageTag = imageTag;
    }).overrideAttrs
      (_old: {
        preferLocalBuild = true;
      });

  unpackDockerImage = hostWriteShellScript "unpack-docker-image" (
    builtins.readFile ./unpack-docker-image.sh
  );
in
stdenv.mkDerivation {
  pname = "open-webui-docker";
  inherit version;

  dontUnpack = true;
  dontPatchELF = true;
  dontStrip = true;
  fixupPhase = "runHook postFixup";

  nativeBuildInputs = [
    unpackJq
    unpackGnutar
    hostPatchelf
  ];

  installPhase = ''
    runHook preInstall
    ${unpackDockerImage} ${image} "$out" \
      ${lib.getExe' unpackGnutar "tar"} ${lib.getExe unpackJq}
    find "$out" -type l ! -exec test -e {} \; -delete

    # hello-llm sets BYPASS_EMBEDDING_AND_RETRIEVAL; drop baked HF/cache payloads.
    rm -rf "$out/app/backend/data/cache"
    find "$out" -type d -name __pycache__ -prune -exec rm -rf {} +
    find "$out" -type f \( -name '*.pyc' -o -name '*.pyo' \) -delete

    mkdir -p "$out/bin"
    cat >"$out/bin/open-webui-portable" <<EOF
    #!/bin/sh
    echo "open-webui-portable: use hello-llm start.sh (bubblewrap)" >&2
    exit 1
    EOF
    chmod +x "$out/bin/open-webui-portable"
    runHook postInstall
  '';

  meta = {
    description = "Open WebUI ${version} rootfs from ghcr.io (linux/${dockerArch})";
    homepage = "https://github.com/open-webui/open-webui";
    license = lib.licenses.unfree;
    platforms = lib.platforms.linux;
    sourceProvenance = [ ];
  };
}
