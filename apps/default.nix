{ sdk, jetpack }:

let
  inherit (sdk.inputs.nixpkgs) lib;
  catalog = import ./examples { inherit lib; };
  builds = sdk.lib.mkAppOutputs {
    self = sdk;
    apps = catalog.apps;
    overlays = [
      jetpack.overlays.default
      (final: prev: {
        nvidia-jetpack = final.nvidia-jetpack7;
        cudaPackages = prev.cudaPackages_13_2;
      })
    ];
  };
in
{
  inherit (builds) packages teka;
  inherit (catalog) templates;
  checks = lib.recursiveUpdate builds.checks (import ./tests { inherit sdk; });
  apps = lib.genAttrs (lib.attrNames sdk.packages) (
    system:
    let
      pkgs = sdk.inputs.nixpkgs.legacyPackages.${system};
    in
    {
      update-cargo-lock = {
        type = "app";
        program = lib.getExe (
          pkgs.writeShellApplication {
            name = "update-cargo-lock";
            runtimeInputs = [ pkgs.cargo ];
            text = ''
              for d in ${lib.escapeShellArgs catalog.cargoDirs}; do
                echo "updating $d/Cargo.lock"
                (cd "$d" && cargo update "$@")
              done
            '';
          }
        );
      };
    }
  );
}
