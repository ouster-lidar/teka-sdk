# aarch64 Nsight Compute target CLI from Jetpack debs (no Qt cross build).
# Pattern from jetpack-nixos pkgs/cuda-packages-11-4/nsight_compute_target.nix.
{ nvidia-jetpack }:

let
  version = "2026.1.1";
  targetArch = "linux-desktop-t210-a64";

  finalAttrs = {
    pname = "nsight-compute-target";
    inherit version;
    srcs = [ nvidia-jetpack.debs.common."nsight-compute-${version}".src ];
    preDebNormalization = ''
      pushd "$NIX_BUILD_TOP/$sourceRoot" >/dev/null
      mv --verbose --no-clobber "$PWD/opt/nvidia/nsight-compute/${version}/target/${targetArch}" "$PWD/target"
      mv --verbose --no-clobber "$PWD/opt/nvidia/nsight-compute/${version}/extras" "$PWD/extras"
      mv --verbose --no-clobber "$PWD/opt/nvidia/nsight-compute/${version}/sections" "$PWD/sections"
      nixLog "removing $PWD/opt"
      rm --recursive --dir "$PWD/opt" || {
        nixErrorLog "$PWD/opt contains non-empty directories: $(ls -laR "$PWD/opt")"
        exit 1
      }
      mkdir -p bin
      ln -sfv ../target/ncu ./bin/ncu
      popd >/dev/null
    '';
    meta.platforms = [ "aarch64-linux" ];
  };
in
nvidia-jetpack.buildFromDebs finalAttrs
