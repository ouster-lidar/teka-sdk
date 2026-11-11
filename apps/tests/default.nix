{ sdk }:

let
  inherit (sdk.inputs.nixpkgs) lib;
  examples = lib.mapAttrs (name: _: import ./example.nix { inherit sdk name; }) (
    builtins.removeAttrs sdk.templates [ "default" ]
  );
in
lib.genAttrs (lib.attrNames sdk.packages) (
  system:
  let
    pkgs = sdk.inputs.nixpkgs.legacyPackages.${system};
  in
  lib.optionalAttrs (lib.hasSuffix "-linux" system) (
    {
      sdk-api = import ./sdk-api.nix { inherit sdk system; };
      flake-output-overlays = pkgs.callPackage ./flake-output-overlays.nix {
        mkAppOutputs = sdk.lib.mkAppOutputs;
        nixpkgs = sdk.inputs.nixpkgs;
        version = sdk.lib.mkVersion {
          self = { };
          inherit (pkgs) lib;
        };
      };
      signed-images = pkgs.callPackage ./signed-images.nix {
        version = sdk.lib.mkVersion {
          self = sdk;
          inherit lib;
        };
      };
      hello-llm-runtime = pkgs.runCommand "hello-llm-runtime-check" { } ''
        TEKA_RUNNER=${sdk.packages.${system}.teka-runner}/bin/teka-runner \
          ${pkgs.python3}/bin/python3 ${../../.}/apps/examples/hello-llm/tests/runtime.py
        touch "$out"
      '';
      hello-ouster-world-startup = pkgs.runCommand "hello-ouster-world-startup-check" { } ''
        ${pkgs.python3}/bin/python3 ${../../.}/apps/examples/hello-ouster-world/tests/startup.py
        touch "$out"
      '';
      example-templates = import ./templates.nix { inherit sdk system; };
    }
    // import ./rust-source.nix { inherit sdk system; }
    // import ./hello-world.nix {
      inherit sdk system;
      example = examples.hello-world;
    }
    // lib.concatMapAttrs (
      name: example:
      let
        packages = example.outputs.teka.apps.${name}.packages.${system};
      in
      lib.mapAttrs' (
        checkName: check: lib.nameValuePair "${name}-example-${checkName}" check
      ) example.outputs.checks.${system}
      // {
        "${name}-example-packages" = pkgs.runCommand "${name}-example-packages-check" { } ''
          ${lib.concatMapStringsSep "\n" (package: ''
            test -e ${package}/*.tpk
          '') (lib.attrValues packages)}
          touch "$out"
        '';
      }
    ) examples
  )
)
