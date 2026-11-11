export PATH="@clientPath@${PATH:+:$PATH}"
# Base GPU library path comes from EnvironmentFile=/etc/teka/wlroots.env; append
# closure-specific dirs (Vulkan, libc, …) baked in at eval time.
export LD_LIBRARY_PATH="${LD_LIBRARY_PATH:+$LD_LIBRARY_PATH:}@clientLd@"
export XDG_RUNTIME_DIR=@xdgRuntimeDir@
export HOME="$XDG_RUNTIME_DIR"
export SHELL=@bash@/bin/bash

unset WAYLAND_DISPLAY DISPLAY 2>/dev/null || true

filter_wlroots_stderr() {
  local _orphan='run\[[0-9]+\]: "[[:space:]]*$'
  local _bare='^[[:space:]]*\"[[:space:]]*$'
  while IFS= read -r line || [[ -n "$line" ]]; do
    case "$line" in
      *eglQueryDmaBufModifiersEXT*) continue ;;
      *'Failed to query dmabuf number of modifiers'*) continue ;;
      *'NVMAP_IOC_PARAMETERS failed'*) continue ;;
    esac
    if [[ "$line" =~ $_orphan ]] || [[ "$line" =~ $_bare ]]; then
      continue
    fi
    printf '%s\n' "$line" >&2
  done
}
