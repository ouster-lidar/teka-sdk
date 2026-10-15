{ pkgs, imageVersion }:

''
  ${pkgs.erofs-utils}/bin/dump.erofs --cat --path=usr/lib/jetstream/mission-control/manifest.json "$raw" >manifest.json
  ${pkgs.jq}/bin/jq -e '
    .id == "hello-ouster-terminal" and .version == "${imageVersion}" and
    .nav.label == "Hello Ouster Terminal" and .nav.path == "/app/hello-ouster-terminal" and
    .ui.mode == "external" and .backend.mode == "none" and
    .portable.displayService == true and .portable.gpu == "wayland"
  ' manifest.json >/dev/null
''
