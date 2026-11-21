{
  sdk,
  system,
  example,
}:

let
  pkgs = sdk.inputs.nixpkgs.legacyPackages.${system};
  program = pkgs.callPackage (example.source + "/package.nix") {
    teka = {
      appId = "renamed-hello";
      lib.mkAppImage = module: pkgs.lib.getExe module.services.main.program;
    };
  };
  packages = example.outputs.packages.${system}.hello-world.targets;
in
{
  hello-world-example-run = pkgs.runCommand "hello-world-run-check" { } ''
    test "$(${program})" = 'hello world'
    touch "$out"
  '';

  hello-world-example-version = pkgs.runCommand "hello-world-version-check" { } ''
    ${pkgs.lib.concatMapStrings (package: ''
      test -f "${package.image}/${pkgs.lib.removeSuffix ".tpk" package.tpkFileName}.raw"
    '') (pkgs.lib.attrValues packages)}
    touch "$out"
  '';
}
