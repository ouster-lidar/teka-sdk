{
  sdk,
  system,
  example,
}:

let
  pkgs = sdk.inputs.app-nixpkgs.legacyPackages.${system};
  program = pkgs.callPackage (import example.source).image {
    ps.makeAppImage = image: builtins.head image.runtimeClosures;
  };
  packages = pkgs.lib.filterAttrs (_: package: package ? image) example.outputs.packages.${system};
in
{
  hello-world-example-run = pkgs.runCommand "hello-world-run-check" { } ''
    test "$(${pkgs.lib.getExe program})" = 'hello world'
    touch "$out"
  '';

  hello-world-example-version = pkgs.runCommand "hello-world-version-check" { } ''
    ${pkgs.lib.concatMapStringsSep "\n" (package: ''
      test -e ${package.image}/*+20260102030405.gabc1234.*.raw
    '') (pkgs.lib.attrValues packages)}
    touch "$out"
  '';
}
