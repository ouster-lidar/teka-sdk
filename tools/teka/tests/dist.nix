{
  lib,
  stdenv,
  runCommand,
  binutils,
  darwin,
  file,
  dist,
  system,
  version,
}:
runCommand "teka-dist-check-${system}"
  {
    nativeBuildInputs = lib.optional stdenv.hostPlatform.isLinux binutils;
  }
  (
    ''
      test "$(${lib.getExe dist} --version)" = 'teka ${version}'
      test "$(${dist}/bin/teka-${system} --version)" = 'teka ${version}'
      cp ${dist}/teka-${system} ${dist}/SHA256SUMS .
      sha256sum --check SHA256SUMS
      test "$(wc -l < SHA256SUMS)" -eq 1
      mv teka-${system} teka
      test "$(./teka --version)" = 'teka ${version}'
      ./teka --help > /dev/null
      for shell in bash fish zsh; do
        ./teka completion "$shell" > "$shell.completion"
        test -s "$shell.completion"
      done
    ''
    + lib.optionalString stdenv.hostPlatform.isLinux ''
      readelf -h teka > header
      grep -E 'Machine:.*${
        if stdenv.hostPlatform.isAarch64 then "AArch64" else "Advanced Micro Devices X86-64"
      }' header
      readelf -l teka > segments
      ! grep -w INTERP segments
      readelf -d teka > dynamic
      ! grep -F '(NEEDED)' dynamic
    ''
    + lib.optionalString stdenv.hostPlatform.isDarwin ''
      ${lib.getExe file} teka > format
      grep -F 'Mach-O 64-bit arm64' format
      ${lib.getExe' darwin.cctools "otool"} -L teka > libraries
      tail -n +2 libraries | awk '$1 !~ /^\/(usr\/lib|System\/Library)\// { exit 1 }'
      ${lib.getExe' darwin.cctools "otool"} -l teka > load-commands
      grep -F 'minos ${stdenv.hostPlatform.darwinMinVersion}' load-commands
    ''
    + ''
      touch "$out"
    ''
  )
