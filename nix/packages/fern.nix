{
  lib,
  runCommand,
  fetchurl,
  nodejs,
}:
let
  version = "5.218.2";
  # npm publishes one bundled cli.cjs and no runtime dependencies.
  tarball = fetchurl {
    url = "https://registry.npmjs.org/fern-api/-/fern-api-${version}.tgz";
    hash = "sha256-0jKZul2B5RcSuAkt85mG9f9xrwkQWrtpckOtNCeBmnc=";
  };
in
runCommand "fern-api-${version}"
  {
    nativeBuildInputs = [ nodejs ];
    meta = {
      description = "Fern CLI";
      homepage = "https://buildwithfern.com/";
      license = lib.licenses.asl20;
      mainProgram = "fern";
    };
  }
  ''
    mkdir -p $out/bin
    tar -xzf ${tarball} --strip-components=1
    install -m755 cli.cjs $out/bin/fern
    patchShebangs $out/bin/fern
  ''
