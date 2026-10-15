# Source tree for app-runtime path dependency overrides (cross-repo consumers).
{ lib, runCommand }:

runCommand "app-runtime-src" { } ''
  cp -r ${lib.cleanSource ../lib/app-runtime} $out
''
