{ pkgs, imageVersion }:

''
  dump=${pkgs.erofs-utils}/bin/dump.erofs
  "$dump" --offset="$root_offset" --cat --path=usr/lib/systemd/system/hello-ouster-terminal.service "$raw" >service
  grep -qxF 'X-Teka-PortableGPU=wayland' service
  grep -qxF 'Type=notify' service
  grep -qE '^After=(.* )?teka-manager\.target( |$)' service
  grep -qE '^ExecStart=.*/teka-runner hello-ouster-terminal udp 7000-8000 ' service
  "$dump" --offset="$root_offset" --cat --path=bin/bash "$raw" >kiosk-bash
  printf '\177ELF' | cmp -n 4 - kiosk-bash
  "$dump" --offset="$root_offset" --cat --path=usr/lib/teka/app/manifest.json "$raw" >manifest.json
  ${pkgs.jq}/bin/jq -e '
    .id == "hello-ouster-terminal" and .version == "${imageVersion}" and
    .nav.label == "Hello Ouster Terminal" and .nav.path == "/app/hello-ouster-terminal" and
    .nav.panels == [{"label":"Display","panel":"display"}] and
    .ui.mode == "static" and (.ui.styles // []) == [] and
    .backend.mode == "none" and
    .portable.displayService == true and .portable.gpu == "wayland"
  ' manifest.json >/dev/null
  "$dump" --offset="$root_offset" --ls --path=usr/lib/teka/app/web "$raw" >web-files
  test "$(awk 'NF == 3 && $2 == 1 { print $3 }' web-files)" = "$(printf '%s\n' app-api.js display.html display.js)"
  "$dump" --offset="$root_offset" --cat --path=usr/lib/teka/app/web/display.html "$raw" >display.html
  grep -q 'HDMI display' display.html
''
