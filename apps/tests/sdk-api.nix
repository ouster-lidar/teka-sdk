{ sdk, system }:

let
  pkgs = sdk.inputs.nixpkgs.legacyPackages.${system};
  crossPkgs = import sdk.inputs.nixpkgs {
    localSystem = system;
    crossSystem = "aarch64-linux";
    overlays = [ sdk.overlays.default ];
  };
in
assert pkgs.lib.all (name: builtins.hasAttr name sdk.inputs) [
  "nixpkgs"
  "crane"
  "jetpack"
  "rustsecAdvisoryDb"
];
assert sdk.packages.${system}.teka-src ? dummySrc;
assert
  builtins.attrNames sdk.lib == [
    "certs"
    "mkAppOutputs"
    "mkErofsImageScript"
    "mkVersion"
  ];
assert sdk.lib.certs.app.dev == builtins.readFile ../../nix/apps/dev-keys/apps.pem;
assert
  builtins.attrNames (sdk.overlays.default crossPkgs crossPkgs) == [ "teka" ]
  && crossPkgs.buildPackages.teka.drvPath == sdk.packages.${system}.teka.drvPath;
assert
  (sdk.lib.mkAppOutputs {
    self = { };
    apps.unbuilt = throw "app registration forced an app value";
  })
    ? teka.apps.unbuilt;
assert
  !(
    (sdk.lib.mkAppOutputs {
      self = { };
      apps = { };
    })
      ? packages
  );
assert
  builtins.attrNames sdk.packages.${system} == [
    "default"
    "fern"
    "portable"
    "publish-cache"
    "teka"
    "teka-runner"
    "teka-src"
  ];
assert
  builtins.attrNames sdk.teka.apps
  == builtins.attrNames (import ../examples { inherit (pkgs) lib; }).apps;
assert
  builtins.attrNames sdk.teka.apps.hello-world.packages == [
    "aarch64-linux"
    "x86_64-linux"
  ];
assert sdk.templates.default.path == sdk.templates.hello-world.path;
pkgs.runCommand "sdk-api" { } ''
  mkdir "source tree"
  echo fixture > "source tree/probe"
  ${sdk.lib.mkErofsImageScript {
    inherit (pkgs.buildPackages) erofs-utils;
    output = ''"$TMPDIR/test image.erofs"'';
    sourceDir = ''"source tree"'';
    extraArgs = [
      "-U"
      "12345678-1234-1234-1234-123456789abc"
    ];
  }}
  ${pkgs.buildPackages.erofs-utils}/bin/dump.erofs --cat --path=probe "$TMPDIR/test image.erofs" | cmp "source tree/probe" -
  if bash -e <<'EOF'
  ${sdk.lib.mkErofsImageScript {
    inherit (pkgs.buildPackages) erofs-utils;
    output = "missing.erofs";
    sourceDir = "missing-source";
  }}
  exit 0
  EOF
  then
    echo "EROFS creation failure did not stop the build script" >&2
    exit 1
  fi
  touch "$out"
''
