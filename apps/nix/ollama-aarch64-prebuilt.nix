# Prebuilt Ollama for aarch64-linux (Jetson / ZED Box Mini).
# nixpkgs ollama does not cross-compile from x86_64; upstream ships binary drops.
{
  lib,
  stdenv,
  buildPackages,
  fetchurl,
}:

let
  version = "0.24.0";
  base = fetchurl {
    url = "https://github.com/ollama/ollama/releases/download/v${version}/ollama-linux-arm64.tar.zst";
    hash = "sha256-bpo85fZOkzEpAuOcQg7DNiVfB4o2jKJembM50Ipt+ks=";
  };
  jetpack6 = fetchurl {
    url = "https://github.com/ollama/ollama/releases/download/v${version}/ollama-linux-arm64-jetpack6.tar.zst";
    hash = "sha256-bP9ELiPCF9BIBs1mDKLJWBuqn7Sc6DTznEdSWT8QEzg=";
  };
in
stdenv.mkDerivation {
  pname = "ollama-aarch64-prebuilt";
  inherit version;

  srcs = [
    base
    jetpack6
  ];
  sourceRoot = ".";

  nativeBuildInputs = [
    buildPackages.zstd
    buildPackages.gnutar
    buildPackages.patchelf
  ];

  unpackPhase = ''
    runHook preUnpack
    mkdir -p base jp6
    zstd -d -c "${base}" | tar -x -C base
    zstd -d -c "${jetpack6}" | tar -x -C jp6
    runHook postUnpack
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p "$out/bin" "$out/lib/ollama"
    install -m755 base/bin/ollama "$out/bin/ollama"
    # Jetson only needs cuda_jetpack6; base tarball also ships cuda_v12/v13 (~3.6 GiB).
    for item in base/lib/ollama/*; do
      base_name=$(basename "$item")
      if [[ "$base_name" != cuda_v* ]]; then
        cp -a "$item" "$out/lib/ollama/"
      fi
    done
    cp -a jp6/lib/ollama/cuda_jetpack6 "$out/lib/ollama/"

    # Upstream uses /lib/ld-linux-aarch64.so.1; portables only see host libs via bind + nix store.
    patchelf --set-interpreter "${stdenv.cc.bintools.dynamicLinker}" "$out/bin/ollama"
    patchelf --set-rpath "${
      lib.makeLibraryPath [
        stdenv.cc.cc.lib
        stdenv.cc.cc
      ]
    }:$out/lib/ollama" "$out/bin/ollama"

    runHook postInstall
  '';

  meta = with lib; {
    description = "Prebuilt Ollama ${version} for Linux arm64 with JetPack 6 CUDA libs";
    homepage = "https://github.com/ollama/ollama";
    license = licenses.mit;
    mainProgram = "ollama";
    # Fetched/unpacked on any builder; binaries run on aarch64 only.
    platforms = lib.platforms.linux;
  };
}
