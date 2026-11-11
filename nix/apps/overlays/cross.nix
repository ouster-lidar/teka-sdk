{ nixpkgs }:
final: prev:
(import ./pipewire-cross-no-modemmanager-overlay.nix) final prev
// {
  onnxruntime = prev.onnxruntime.overrideAttrs (old: {
    cmakeFlags =
      let
        flags = old.cmakeFlags or [ ];
      in
      (builtins.filter (
        f: !(builtins.isString f && prev.lib.strings.hasInfix "ONNX_CUSTOM_PROTOC_EXECUTABLE" f)
      ) flags)
      ++ [
        (prev.lib.cmakeFeature "ONNX_CUSTOM_PROTOC_EXECUTABLE" (
          prev.lib.getExe prev.buildPackages.protobuf
        ))
      ];
  });
  openblas =
    if
      final.stdenv.hostPlatform.isLinux
      && final.stdenv.hostPlatform.isAarch64
      && final.stdenv.hostPlatform.system != final.stdenv.buildPlatform.system
    then
      prev.openblas.override { dynamicArch = false; }
    else
      prev.openblas;
  # FIXME: Remove when the pin includes https://github.com/NixOS/nixpkgs/pull/577654.
  # Legacy GCC 16 omits the existing libgomp UID buffer fix and fails with -Werror=format-truncation.
  gfortran =
    if prev.stdenv.hostPlatform.system != prev.stdenv.buildPlatform.system then
      prev.wrapCC (
        prev.gfortran.cc.overrideAttrs (old: {
          patches = old.patches ++ [
            (nixpkgs + "/pkgs/development/compilers/gcc/ng/16/libgomp/uid-buffer-size.patch")
          ];
        })
      )
    else
      prev.gfortran;
}
