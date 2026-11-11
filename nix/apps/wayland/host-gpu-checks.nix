# Small fail-fast helpers for app GPU services: host generator paths under
# $TEKA_OPENGL_DRIVER (default /run/opengl-driver) and seatd for Wayland.
# Units invoke these from ExecStartPre= before ExecStart=.
{
  lib,
  symlinkJoin,
  writeShellScriptBin,
}:

let
  # teka-portable-check-vulkan-icds — Vulkan ICD directory (VK_DRIVER_FILES).
  checkVulkanIcds = writeShellScriptBin "teka-portable-check-vulkan-icds" ''
    set -euo pipefail
    die() { echo "''${TEKA_PORTABLE_LOG_TAG:-teka-portable}: $*" >&2; exit 1; }
    root="''${TEKA_OPENGL_DRIVER:-/run/opengl-driver}"
    icd="$root/share/vulkan/icd.d"
    [[ -d "$icd" ]] \
      || die "missing $icd (VK_DRIVER_FILES; teka-portable-gpu-generator?)"
  '';

  # teka-portable-check-glvnd-egl-vendors — GLVND EGL vendor JSON dir (__EGL_VENDOR_LIBRARY_DIRS).
  checkGlvndEglVendors = writeShellScriptBin "teka-portable-check-glvnd-egl-vendors" ''
    set -euo pipefail
    die() { echo "''${TEKA_PORTABLE_LOG_TAG:-teka-portable}: $*" >&2; exit 1; }
    root="''${TEKA_OPENGL_DRIVER:-/run/opengl-driver}"
    egl="$root/share/glvnd/egl_vendor.d"
    [[ -d "$egl" ]] \
      || die "missing $egl (__EGL_VENDOR_LIBRARY_DIRS; teka-portable-gpu-generator?)"
  '';

  checkSeatdSocket = writeShellScriptBin "teka-portable-check-seatd-socket" ''
    set -euo pipefail
    die() { echo "''${TEKA_PORTABLE_LOG_TAG:-teka-portable}: $*" >&2; exit 1; }
    [[ -S /run/seatd.sock ]] \
      || die "missing /run/seatd.sock (enable host seatd.service)"
  '';

  # wlroots DRM/KMS compositors need a usable GBM backend under the merged opengl-driver tree.
  checkGbmBackends = writeShellScriptBin "teka-portable-check-gbm-backends" ''
    set -euo pipefail
    die() { echo "''${TEKA_PORTABLE_LOG_TAG:-teka-portable}: $*" >&2; exit 1; }
    root="''${TEKA_OPENGL_DRIVER:-/run/opengl-driver}"
    default_gbm="$root/lib/gbm"
    search="''${GBM_BACKENDS_PATH:-$default_gbm}"
    found=
    IFS=:
    for gbm_dir in $search; do
      [[ -n "$gbm_dir" ]] || continue
      [[ -d "$gbm_dir" ]] || continue
      for cand in nvidia-drm_gbm.so tegra_gbm.so tegra-udrm_gbm.so dri_gbm.so; do
        f="$gbm_dir/$cand"
        if [[ -L "$f" ]] || { [[ -f "$f" ]] && [[ -s "$f" ]]; }; then
          found=1
          break 2
        fi
      done
    done
    [[ -n "$found" ]] \
      || die "no GBM backend under GBM_BACKENDS_PATH (default $default_gbm; nvidia-drm/tegra/dri)"
  '';
in
symlinkJoin {
  name = "teka-portable-host-gpu-checks";
  paths = [
    checkVulkanIcds
    checkGlvndEglVendors
    checkSeatdSocket
    checkGbmBackends
  ];
  meta = with lib; {
    description = "Host GPU path assertions for Teka app images";
    license = licenses.mit;
  };
}
