{ pkgs, imageVersion }:
let
  inherit (pkgs.lib.importJSON ../manifest.json) id;
in
''
  extract() {
    ${pkgs.erofs-utils}/bin/dump.erofs --offset="$root_offset" --cat --path="$1" "$raw"
  }

  extract usr/lib/systemd/system/${id}.service >api
  extract usr/lib/systemd/system/${id}-display.service >display
  for unit in api display; do
    script=$(sed -n 's/^ExecStart=//p' "$unit")
    test -n "$script"
    extract "''${script#/}" >launcher
    test -s launcher
    grep -qxF 'WantedBy=teka-apps.target' "$unit"
    grep -qxF 'NetworkNamespacePath=/proc/1/ns/net' "$unit"
    grep -qxF 'BindPaths=/srv' "$unit"
    grep -qxF 'RuntimeDirectory=${id}' "$unit"
  done
  grep -qxF 'X-Teka-PortableGPU=cuda' api
  grep -qxF 'Type=notify' api
  grep -qE '^After=(.* )?teka-manager\.target( |$)' api
  grep -qxF 'X-Teka-PortableGPU=wayland' display
  grep -qxF 'Requires=${id}.service' display
  grep -qE '^After=(.* )?seatd\.service( |$)' display
  grep -qxF 'EnvironmentFile=/etc/teka/wlroots.env' display
  grep -qF 'teka-portable-check-vulkan-icds' display

  extract etc/dbus-1/session.conf >dbus.conf
  test -s dbus.conf
  sway=$(sed -n 's/.* --config \([^ ]*\) .*/\1/p' launcher)
  startup=$(extract "''${sway#/}" | sed -n 's/^exec //p')
  viz=$(extract "''${startup#/}" | sed -n 's/^  \(\/nix\/store\/[^ ]*\) || true$/\1/p')
  extract "''${viz#/}" >viz.sh
  grep -Fx 'export GLFW_WAYLAND_APP_ID=hello-ouster-world' viz.sh
  viewer=$(sed -n 's/.*\/bin\/python3 \([^ ]*\) "\$@"/\1/p' viz.sh)
  extract "''${viewer#/}" >viewer.py
  cmp viewer.py ${../viewer.py}
  extract usr/lib/teka/app/manifest.json | ${pkgs.jq}/bin/jq -e '
    .id == "${id}" and .version == "${imageVersion}" and
    .portable.displayService and .portable.gpu == "wayland"
  '
  extract usr/lib/teka/app/web/display.html | cmp ${../web/display.html} -
  extract usr/lib/teka/app/web/display.js >display.js
  cmp display.js ${../web/display.js}
  for name in app-api.js xterm.js xterm.css xterm-addon-fit.js; do
    extract "usr/lib/teka/app/web/$name" >asset
    test -s asset
  done
''
