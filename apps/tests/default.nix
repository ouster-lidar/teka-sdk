{ sdk }:

let
  inherit (sdk.inputs.nixpkgs) lib;
  examples = lib.mapAttrs (name: _: import ./example.nix { inherit sdk name; }) (
    builtins.removeAttrs sdk.templates [ "default" ]
  );
in
lib.genAttrs (lib.attrNames sdk.packages) (
  system:
  lib.optionalAttrs (lib.hasSuffix "-linux" system) (
    {
      sdk-api = import ./sdk-api.nix { inherit sdk system; };
      example-templates = import ./templates.nix { inherit sdk system; };
    }
    // import ./hello-world.nix {
      inherit sdk system;
      example = examples.hello-world;
    }
    // lib.concatMapAttrs (
      name: example:
      let
        pkgs = sdk.inputs.nixpkgs.legacyPackages.${system};
        packages = example.outputs.packages.${system};
        crossPackages = lib.filterAttrs (
          packageName: _: lib.hasPrefix "${name}-cross-" packageName
        ) packages;
      in
      lib.mapAttrs' (
        checkName: check: lib.nameValuePair "${name}-example-${checkName}" check
      ) example.outputs.checks.${system}
      // {
        "${name}-example-packages" = pkgs.runCommand "${name}-example-packages-check" { } ''
          ${lib.concatMapStringsSep "\n" (package: ''
            test -e ${package}/*.tpk
          '') ([ packages.${name} ] ++ lib.attrValues crossPackages)}
          touch "$out"
        '';
      }
    ) examples
  )
)
