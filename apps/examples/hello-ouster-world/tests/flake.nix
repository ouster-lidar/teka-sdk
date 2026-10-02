{
  description = "hello-ouster-world device tests";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/a50de1b7d8a586adc18d2395c19de7d6058e6030";
  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          python = pkgs.python3.withPackages (ps: [
            ps.pytest
            ps.pytest-playwright
            ps.pillow
          ]);
          browsers = pkgs.playwright-driver.browsers.override {
            withFirefox = false;
            withWebkit = false;
            withFfmpeg = false;
          };
        in
        {
          default = pkgs.writeShellApplication {
            name = "test-hello-ouster-world";
            runtimeInputs = with pkgs; [
              bash
              coreutils
              curl
              gnugrep
              gnused
              jq
              nix
            ];
            text = ''
              export PLAYWRIGHT_BROWSERS_PATH=${browsers}
              export PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=1
              export JETSTREAM_TEST_DEPLOY_SCRIPT=${../../../scripts}/jsd-app-deploy.sh
              exec ${python}/bin/python -m pytest ${./.} --rootdir ${./.} \
                -p no:cacheprovider --output tmp/test-hello-ouster-world --tracing retain-on-failure --screenshot only-on-failure "$@"
            '';
          };
        }
      );
      apps = forAllSystems (system: {
        default = self.apps.${system}.test-hello-ouster-world;
        test-hello-ouster-world = {
          type = "app";
          program = nixpkgs.lib.getExe self.packages.${system}.default;
        };
      });
    };
}
