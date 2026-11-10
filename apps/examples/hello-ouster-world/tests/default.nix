{
  bash,
  nix,
  playwright-driver,
  python3,
  teka,
  writeShellApplication,
}:

let
  python = python3.withPackages (ps: [
    ps.pytest
    ps.pytest-playwright
    ps.pillow
  ]);
  browsers = playwright-driver.browsers.override {
    withFirefox = false;
    withWebkit = false;
    withFfmpeg = false;
  };
in
writeShellApplication {
  name = "test-hello-ouster-world";
  runtimeInputs = [
    bash
    nix
    teka
  ];
  text = ''
    export PLAYWRIGHT_BROWSERS_PATH=${browsers}
    export PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=1
    exec ${python}/bin/python -m pytest ${./.} --rootdir ${./.} \
      -p no:cacheprovider --output tmp/test-hello-ouster-world --tracing retain-on-failure --screenshot only-on-failure "$@"
  '';
}
