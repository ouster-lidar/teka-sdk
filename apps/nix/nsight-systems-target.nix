# aarch64 Nsight Systems target CLI from Jetpack debs (no Qt cross build).
# Pattern from jetpack-nixos pkgs/cuda-packages-11-4/nsight_systems_target.nix.
{
  nvidia-jetpack,
  lib,
  libbpf,
  libcap,
  openssl,
  zlib,
}:

let
  version = "2025.6.3";
  targetArch = "target-linux-sbsa-armv8";

  finalAttrs = {
    pname = "nsight-systems-target";
    inherit version;
    srcs = [ nvidia-jetpack.debs.common."nsight-systems-${version}".src ];
    buildInputs = [
      libbpf
      libcap
      openssl
      zlib
    ];
    autoPatchelfIgnoreMissingDeps = [
      "libibmad.so.5"
      "libibumad.so.3"
      "libibverbs.so.1"
      "libnvidia-ml.so.1"
    ];
    preDebNormalization = ''
      pushd "$NIX_BUILD_TOP/$sourceRoot" >/dev/null
      mv --verbose --no-clobber "$PWD/opt/nvidia/nsight-systems/${version}/${targetArch}" "$PWD/${targetArch}"
      rm -rf \
        "$PWD/${targetArch}/CollectX" \
        "$PWD/opt/nvidia/nsight-systems/${version}/documentation"
      nixLog "removing $PWD/opt"
      rm --recursive --dir "$PWD/opt" || {
        nixErrorLog "$PWD/opt contains non-empty directories: $(ls -laR "$PWD/opt")"
        exit 1
      }
      mkdir -p bin
      ln -sfv ../${targetArch}/nsys ./bin/nsys
      ln -sfv ../${targetArch}/nsys-launcher ./bin/nsys-launcher
      popd >/dev/null
    '';
    meta.platforms = [ "aarch64-linux" ];
  };
in
nvidia-jetpack.buildFromDebs finalAttrs
