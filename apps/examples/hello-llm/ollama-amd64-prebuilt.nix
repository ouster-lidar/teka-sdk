{
  stdenvNoCC,
  stdenv,
  fetchurl,
  autoPatchelfHook,
  zstd,
}:
stdenvNoCC.mkDerivation (finalAttrs: {
  pname = "ollama-amd64-prebuilt";
  version = "0.40.16";

  src = fetchurl {
    url = "https://github.com/ollama/ollama/releases/download/v${finalAttrs.version}/ollama-linux-amd64.tar.zst";
    hash = "sha256-h7+vsqy8ibLmob4KkEPeaqdftFBpb4a8Qm5l/ESPeI0=";
  };

  nativeBuildInputs = [
    autoPatchelfHook
    zstd
  ];
  buildInputs = [ stdenv.cc.cc.lib ];
  dontUnpack = true;
  installPhase = ''
    runHook preInstall
    mkdir -p "$out"
    # The x86 service uses the CPU runner, like the native Nixpkgs package.
    tar --zstd -xf "$src" -C "$out" --exclude='*/cuda_v*' --exclude='*/vulkan'
    runHook postInstall
  '';
  meta.mainProgram = "ollama";
})
