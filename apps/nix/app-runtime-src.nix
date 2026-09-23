# Source tree for crate2nix app-runtime path-dep overrides (cross-repo consumers).
{ lib, runCommand }:

runCommand "app-runtime-src" { } ''
  cp -r ${lib.cleanSource ../lib/app-runtime} $out
''
