{ sdk, system }:

let
  pkgs = sdk.inputs.nixpkgs.legacyPackages.${system};
  inherit (pkgs) lib;
  teka = import ../../nix/apps/app-scope.nix {
    inherit pkgs;
    name = "wayland-library-path-check";
    modules = [ ];
    craneLib = sdk.inputs.crane.mkLib pkgs;
    manifest = ./fixtures/manifest.json;
    inherit (lib.importJSON ./fixtures/manifest.json) version;
    buildId = "20260102030405.gabc1234.nf16d05e";
    artifact = "wayland-library-path-check.raw";
  };
  waylandEnvProbe =
    clientPath: clientLd:
    pkgs.writeShellScript "wayland-library-path-check" ''
      set -eu
      ${teka.lib.wayland.mkShellEnv { inherit clientPath clientLd; }}
      exec ${pkgs.lib.getExe' pkgs.coreutils "printenv"} PATH LD_LIBRARY_PATH
    '';
  clientEnvProbe = waylandEnvProbe "/app/bin" "/app/lib";
  hostEnvProbe = waylandEnvProbe "" "";
  literalEnvProbe = waylandEnvProbe "/app bin" "$ORIGIN/lib";
  crossPkgs = import sdk.inputs.nixpkgs {
    localSystem = system;
    crossSystem = "aarch64-linux";
    overlays = [ sdk.overlays.default ];
  };
  acceptsChecks =
    systems: recipes:
    (builtins.tryEval (
      builtins.attrNames
        (sdk.lib.mkAppOutputs {
          self = { };
          inherit systems;
          apps = map (package: { inherit package; }) recipes;
        }).checks.x86_64-linux
    )).success;
  fixture = ./fixtures/app.nix;
  package =
    (sdk.lib.mkAppOutputs {
      self = {
        lastModifiedDate = "20260102030405";
        shortRev = "0000000";
        dirtyShortRev = "abc1234-dirty";
        narHash = "fixture";
      };
      systems = [ system ];
      apps = [
        {
          package = fixture;
          modules = [ { services.main.systemd.serviceConfig.ExecStart = "/bin/true"; } ];
        }
      ];
    }).packages.${system}.packaging-probe;
in
assert pkgs.lib.all (name: builtins.hasAttr name sdk.inputs) [
  "nixpkgs"
  "crane"
  "jetpack"
  "rustsecAdvisoryDb"
];
assert sdk.packages.${system}.teka-src ? dummySrc;
assert acceptsChecks [ "x86_64-linux" ] [ fixture ];
assert !(acceptsChecks [ "x86_64-linux" ] [ ./fixtures/missing-manifest/package.nix ]);
assert !(acceptsChecks [ "x86_64-linux" ] [ ./fixtures/missing-id/package.nix ]);
assert !(acceptsChecks [ "x86_64-linux" ] [ ./fixtures/missing-version/package.nix ]);
assert !(acceptsChecks [ "x86_64-linux" ] [ ./fixtures/invalid-id/package.nix ]);
assert !(acceptsChecks [ "x86_64-linux" ] [ ./fixtures/invalid-version/package.nix ]);
assert !(acceptsChecks [ "x86_64-linux" ] [ ./fixtures/reserved-id/package.nix ]);
assert !(acceptsChecks [ "x86_64-linux" ] [ fixture fixture ]);
assert !(acceptsChecks [ ] [ fixture ]);
assert !(acceptsChecks [ "x86_64-linux" "aarch64-darwin" ] [ fixture ]);
assert !(acceptsChecks [ "x86_64-linux" "x86_64-linux" ] [ fixture ]);
assert
  !(acceptsChecks
    [ "x86_64-linux" "aarch64-linux" ]
    [ fixture ./fixtures/cross-check-name/package.nix ]
  );
assert
  builtins.attrNames sdk.lib == [
    "certs"
    "mkAppOutputs"
    "mkErofsImageScript"
  ];
assert sdk.lib.certs.app.dev == builtins.readFile ../../nix/apps/dev-keys/apps.pem;
assert
  system != "x86_64-linux"
  || (
    crossPkgs.teka.system == system
    && crossPkgs.teka.CARGO_BUILD_TARGET == crossPkgs.stdenv.hostPlatform.rust.rustcTarget
    && builtins.attrNames (sdk.overlays.default crossPkgs crossPkgs) == [ "teka" ]
    && crossPkgs.buildPackages.teka.drvPath == sdk.packages.${system}.teka.drvPath
  );
assert
  builtins.attrNames sdk.packages.${system} == builtins.sort builtins.lessThan (
    [
      "default"
      "fern"
      "portable"
      "publish-cache"
      "teka"
      "teka-runner"
      "teka-src"
    ]
    ++ builtins.attrNames sdk.teka.apps
  );
assert sdk.packages.${system}.default.drvPath == sdk.packages.${system}.teka.drvPath;
assert package.pname == "packaging-probe";
assert package.version == "1.2.3-rc.1";
assert
  package.tpkFileName == "packaging-probe-${system}-v1.2.3-rc.1+20260102030405.gabc1234.nf16d05e.tpk";
assert sdk.packages.${system}.hello-world.version == "0.1.0-alpha.1";
assert sdk.teka.apps.hello-firefox.path == "apps/examples/hello-firefox";
assert
  let
    apps =
      (sdk.lib.mkAppOutputs {
        self.outPath = ./fixtures;
        apps = [
          { package = fixture; }
          { package = ../examples/hello-world/package.nix; }
        ];
      }).teka.apps;
  in
  apps.packaging-probe.path == "." && apps.hello-world.path == null;
assert builtins.attrNames sdk.teka.apps.hello-world == [ "path" ];
assert sdk.templates.default.path == sdk.templates.hello-world.path;
pkgs.runCommand "sdk-api" { } ''
  PATH=/host/bin LD_LIBRARY_PATH=/host/driver/lib ${clientEnvProbe} > inherited-env
  printf '%s\n' /app/bin:/host/bin /host/driver/lib:/app/lib | diff -u - inherited-env
  env -u LD_LIBRARY_PATH PATH=/host/bin ${clientEnvProbe} > unset-library-path
  printf '%s\n' /app/bin:/host/bin /app/lib | diff -u - unset-library-path
  PATH=/host/bin LD_LIBRARY_PATH=/host/driver/lib ${hostEnvProbe} > host-env
  printf '%s\n' /host/bin /host/driver/lib | diff -u - host-env
  env -u LD_LIBRARY_PATH PATH=/host/bin ${literalEnvProbe} > literal-env
  printf '%s\n' '/app bin:/host/bin' '$ORIGIN/lib' | diff -u - literal-env

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
