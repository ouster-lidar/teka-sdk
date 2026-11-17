{ pkgs, imageVersion }:

''
  dump=${pkgs.erofs-utils}/bin/dump.erofs
  "$dump" --offset="$root_offset" --cat --path=usr/lib/teka/mission-control/manifest.json "$raw" >manifest.json
  ${pkgs.jq}/bin/jq -e '
    .id == "hello-ouster-terminal" and .version == "${imageVersion}" and
    .nav.label == "Hello Ouster Terminal" and .nav.path == "/app/hello-ouster-terminal" and
    .nav.panels == [{"label":"Display","panel":"display"}] and
    .ui.mode == "static" and .ui.template == "display.html" and (.ui.styles // []) == [] and
    .backend.mode == "none" and
    .portable.displayService == true and .portable.gpu == "wayland"
  ' manifest.json >/dev/null
  "$dump" --offset="$root_offset" --ls --path=usr/lib/teka/mission-control/web "$raw" >web-files
  test "$(awk 'NF == 3 && $2 == 1 { print $3 }' web-files)" = "$(printf '%s\n' app-api.js display.html display.js)"
  "$dump" --offset="$root_offset" --cat --path=usr/lib/teka/mission-control/web/display.html "$raw" >display.html
  grep -q 'HDMI display' display.html
''
