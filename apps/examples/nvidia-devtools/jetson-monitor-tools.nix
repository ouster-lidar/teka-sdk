# Jetson CPU/GPU monitoring CLIs from L4T debs (tegrastats, jetson_clocks, nvidia-smi).
# Host tegra sysfs + NVML libs come from the Jetstream OS — not bundled here.
{
  pkgs,
  nvidia-jetpack,
}:

pkgs.symlinkJoin {
  name = "jetson-monitor-tools";
  paths = [
    nvidia-jetpack.l4t-tools
    nvidia-jetpack.l4t-nvml
  ];
  meta.mainProgram = "tegrastats";
}
