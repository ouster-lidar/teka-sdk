# CUDA developer / profiling tools for Jetstream aarch64 targets.
# Cross x86→aarch64: ncu from Jetpack debs (nsight-compute-target.nix).
# libcuda comes from the host via X-Jetstream-PortableGPU=cuda — not bundled here.
{
  pkgs,
  lib,
}:

let
  cuda = pkgs.cudaPackages;
  isCross = pkgs.stdenv.hostPlatform.system != pkgs.stdenv.buildPlatform.system;
  isAarch64 = pkgs.stdenv.hostPlatform.system == "aarch64-linux";

  nsightCompute =
    if isCross && isAarch64 then
      pkgs.callPackage ./nsight-compute-target.nix {
        nvidia-jetpack = pkgs.nvidia-jetpack;
      }
    else if !isCross then
      cuda.nsight_compute
    else
      null;

  nsightSystems =
    if isCross && isAarch64 then
      pkgs.callPackage ./nsight-systems-target.nix {
        nvidia-jetpack = pkgs.nvidia-jetpack;
      }
    else if !isCross then
      cuda.nsight_systems
    else
      null;

  jetsonMonitor =
    if isAarch64 then
      pkgs.callPackage ./jetson-monitor-tools.nix {
        nvidia-jetpack = pkgs.nvidia-jetpack;
      }
    else
      null;

  # ncu/nsys debs ship libnvperf; skip standalone CUPTI (~127MiB).
  toolPaths = lib.filter (x: x != null) (
    [
      nsightCompute
      nsightSystems
      jetsonMonitor
      cuda.cuda_sanitizer_api
      cuda.cuda_cuobjdump
      cuda.cuda_nvdisasm
    ]
    ++ lib.optionals (!isCross) [
      cuda.cuda_gdb
    ]
  );
in

pkgs.symlinkJoin {
  name = "nvidia-devtools";
  paths = toolPaths;
  meta = {
    description = "NVIDIA CUDA developer and profiling tools (Nsight, sanitizer)";
    mainProgram = "ncu";
  };
}
