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
}
