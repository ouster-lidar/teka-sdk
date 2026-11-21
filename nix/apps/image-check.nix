{
  pkgs,
  checkName,
  serviceName,
  targetSystem,
  raw,
  manifest,
  imageVersion,
  buildId,
  imageCheck,
}:
let
  rootType =
    {
      x86_64-linux = "4f68bce3-e8cd-4db1-96e7-fbcaf984b709";
      aarch64-linux = "b921b045-1df0-41c3-af44-4c6f280d3fae";
    }
    .${targetSystem};
  dumpFile =
    path: ''${pkgs.erofs-utils}/bin/dump.erofs --offset="$root_offset" --cat --path=${path} "$raw"'';
in
pkgs.runCommand checkName { } ''
  raw="${raw}"
  test -f "$raw"
  root_offset=$(${pkgs.util-linux}/bin/sfdisk --json "$raw" | ${pkgs.jq}/bin/jq -er '.partitiontable | .sectorsize as $sector | .partitions[] | select((.type | ascii_downcase) == "${rootType}") | .start * $sector')
  ${pkgs.erofs-utils}/bin/fsck.erofs --offset="$root_offset" "$raw" >/dev/null
  ${dumpFile "etc/os-release"} >os-release
  ${dumpFile "usr/lib/teka/app/manifest.json"} | cmp ${pkgs.lib.escapeShellArg "${manifest}"} -
  ${dumpFile "usr/lib/systemd/system/${serviceName}.service"} | ${pkgs.gnugrep}/bin/grep . >/dev/null
  expect_line() {
    ${pkgs.gnugrep}/bin/grep -qxF "$1" os-release || {
      echo "app image check ${serviceName}: missing os-release line: $1" >&2
      cat os-release >&2
      exit 1
    }
  }
  ${imageCheck { inherit pkgs imageVersion; }}
  expect_line "ID=${serviceName}"
  expect_line "IMAGE_ID=${serviceName}"
  expect_line 'IMAGE_VERSION="${imageVersion}"'
  expect_line 'VERSION="${imageVersion}"'
  expect_line 'VERSION_ID=1'
  expect_line 'BUILD_ID="${buildId}"'
  touch "$out"
''
