{ sdk, system }:

let
  pkgs = sdk.inputs.nixpkgs.legacyPackages.${system};
  name = "packaging-probe";
  target = if system == "x86_64-linux" then "aarch64-linux" else "x86_64-linux";
  webSrc = ../examples/hello-webui/web;
  web = pkgs.runCommand "packaging-probe-web" { } ''
    install -Dm644 -t "$out/web" ${webSrc}/{demo.css,demo.js}
    cp "$out/web/demo.css" "$out/extra.css"
  '';
  empty = builtins.toFile "empty" "";
  rootTool = pkgs.writeScriptBin "probe-root-tool" "exit 0";
  rootFiles = pkgs.runCommand "packaging-probe-root" { } ''
    mkdir -p "$out/etc/probe"
    echo fixture > "$out/etc/probe/from-directory"
    echo replaced > "$out/etc/probe/empty file"
    ln -s from-directory "$out/etc/probe/replaced-link"
    echo 'exec ${lib.getExe rootTool}' > "$out/etc/probe/launcher"
  '';
  tool = pkgs.writeShellScriptBin "true" "exit 0";
  program = pkgs.writeScriptBin "packaging-probe-main" ''
    #!/bin/sh
    exit 0
  '';
  shellTools = pkgs.buildEnv {
    name = "probe-shell-tools";
    paths = [ tool ];
    pathsToLink = [ "/bin" ];
  };
  outputs = sdk.lib.mkAppOutputs {
    self = { };
    systems = [
      system
      target
    ];
    apps = lib.singleton {
      package = ./fixtures/app.nix;
      modules = [
        (
          {
            config,
            teka,
            pkgs,
            ...
          }:
          assert teka.lib.wayland.dbus.drvPath == pkgs.dbus-session.drvPath;
          {
            services.main = {
              inherit program;
              runtime.managed = true;
              systemd = {
                after = [ "network-online.target" ];
                before = [ "probe.target" ];
                environment.PROBE_PACKAGE = program;
                path = [ program ];
                preStart = "true";
              };
            };
            services.companion = {
              inherit program;
              runtime.network = "private";
              systemd.after = [ config.services.main.unitName ];
              systemd.serviceConfig = {
                ExecStart = lib.mkDefault "/bin/true";
                Environment = "PACKAGING_PROBE=raw";
                SyslogIdentifier = "packaging-companion";
              };
            };
            image = {
              packages = [
                pkgs.dockerTools.caCertificates
                pkgs.fakeNss
                rootFiles
                (pkgs.linkFarm "packaging-probe-web-root" { "${teka.appDir}/web" = web; })
                teka.lib.webApi
                (pkgs.linkFarm "packaging-probe-overrides" {
                  "etc/hosts" = pkgs.writeText "probe-hosts" "127.0.0.1 localhost\n";
                  "etc/passwd" = pkgs.writeText "probe-passwd" "fixture\n";
                  "etc/probe/empty file" = empty;
                  "etc/probe/replaced-link" = empty;
                })
                shellTools
              ];
            };
          }
        )
      ];
      imageCheck = { pkgs, imageVersion }: ''
        test "${imageVersion}" = 1.2.3-rc.1

        extract() {
          ${pkgs.erofs-utils}/bin/dump.erofs --offset="$root_offset" --cat --path="$1" "$raw"
        }

        app_dir=usr/lib/teka/app
        extract usr/lib/systemd/system/${name}.service > unit
        extract etc/systemd/system/${name}.service | cmp unit -
        grep -Fx 'Wants=teka-manager.target' unit
        grep '^After=' unit | grep -F 'teka-manager.target' | grep -F 'network-online.target'
        grep -Fx 'Before=probe.target' unit
        grep -Fx 'Description=Teka App ${name}' unit
        grep -Fx 'WantedBy=teka-apps.target' unit
        grep -Fx 'Type=notify' unit
        grep -Fx 'NotifyAccess=main' unit
        grep -Fx 'ExecStart=${lib.getExe program}' unit
        grep -Fx 'SyslogIdentifier=%N' unit
        grep -Fx 'Environment="APP_ID=${name}"' unit
        grep -Fx 'Environment="PROBE_PACKAGE=${program}"' unit
        grep -Fx 'Environment="PATH=${program}/bin:${program}/sbin"' unit
        grep -E '^ExecStartPre=/nix/store/[^/]+/bin/${name}-pre-start$' unit
        extract usr/lib/systemd/system/${name}-companion.service > companion
        grep -Fx 'After=${name}.service' companion
        test "$(grep '^ExecStart=' companion)" = 'ExecStart=/bin/true'
        grep -Fx 'PrivateNetwork=true' companion
        grep -Fx 'SyslogIdentifier=packaging-companion' companion
        grep -Fx 'Environment="APP_ID=${name}"' companion
        grep -Fx 'Environment=PACKAGING_PROBE=raw' companion
        grep -Fx 'IPAddressAllow=localhost' unit
        if grep -Fq 'EnvironmentFile=' unit; then
          exit 1
        fi
        extract "$app_dir/web/app-api.js" > app-api.js
        cmp ${../../nix/apps/web/app-api.js} app-api.js
        ${pkgs.erofs-utils}/bin/dump.erofs --offset="$root_offset" --ls --path=nix/store "$raw" > store-files
        if awk 'NF == 3 { print $3 }' store-files |
          grep -Fx -e "${baseNameOf (toString web)}" -e "${baseNameOf (toString tool)}"
        then
          echo "Build-only package was copied into the embedded store" >&2
          exit 1
        fi
        extract "$app_dir/web/web/demo.js" > demo.js
        cmp ${webSrc}/demo.js demo.js
        ${pkgs.erofs-utils}/bin/dump.erofs --offset="$root_offset" --ls --path="$app_dir/web" "$raw" > web-files
        ${pkgs.erofs-utils}/bin/dump.erofs --offset="$root_offset" --ls --path="$app_dir/web/web" "$raw" >> web-files
        if grep -Eq '[[:space:]](flake\.nix|demo\.html)$' web-files; then
          exit 1
        fi
        for css in web/demo.css extra.css; do
          extract "$app_dir/web/$css" > asset.css
          cmp ${webSrc}/demo.css asset.css
        done

        extract etc/ssl/certs/ca-bundle.crt > ca.pem
        grep -Fx -- '-----BEGIN CERTIFICATE-----' ca.pem >/dev/null
        extract etc/ssl/certs/ca-certificates.crt | cmp ca.pem -
        extract etc/pki/tls/certs/ca-bundle.crt | cmp ca.pem -
        extract etc/hosts | grep -Fx '127.0.0.1 localhost'
        extract etc/passwd | grep -Fx fixture
        extract etc/nsswitch.conf | grep -Fx 'hosts: files dns'

        extract etc/probe/from-directory | grep -Fx fixture
        extract etc/probe/replaced-link > replaced-link
        test ! -s replaced-link
        extract etc/probe/launcher | grep -Fx 'exec ${lib.getExe rootTool}'
        extract "${lib.removePrefix "/" (lib.getExe rootTool)}" | cmp ${lib.getExe rootTool} -
        extract bin/true | cmp ${lib.getExe tool} -
        extract "nix/store/${builtins.baseNameOf "${program}"}/bin/packaging-probe-main" > main-program
        cmp ${lib.getExe program} main-program

        for path in var/lib/${name}/.keep 'etc/probe/empty file'; do
          extract "$path" > empty
          test ! -s empty
        done
      '';
    };
  };
  inherit (pkgs) lib;
  inherit (outputs.packages.${system}.${name}.targets.${system}) image;
  extend = module: image.override { modules = [ module ]; };
  rejected = module: !(builtins.tryEval (extend module).drvPath).success;
in
assert (extend { services.main.systemd.wantedBy = [ "probe.target" ]; }).drvPath != image.drvPath;
assert (extend { services.main.systemd.script = "true"; }).drvPath != image.drvPath;
assert (extend { services.main.runtime.network = lib.mkDefault "host"; }).drvPath != image.drvPath;
assert
  (extend { services.main.systemd.serviceConfig.Type = lib.mkDefault "simple"; }).drvPath
  != image.drvPath;
assert rejected {
  services.main.systemd.serviceConfig.ExecStart = lib.mkForce [ ];
};
assert rejected {
  services.main.systemd.serviceConfig = {
    ExecStart = lib.mkForce [ ];
    ExecStop = "/bin/true";
  };
};
assert rejected {
  services.stop-only.systemd.serviceConfig.ExecStop = "/bin/true";
};
assert rejected { services.main.unitName = "unexpected.service"; };
assert rejected { services.main.systemd.name = lib.mkForce "unexpected.service"; };
assert rejected { services."invalid.role".program = program; };
assert rejected {
  services.${lib.concatStrings (lib.replicate 256 "a")}.program = program;
};
assert (extend { image.packages = [ pkgs.emptyDirectory ]; }).drvPath != image.drvPath;

{
  app-packaging-api = outputs.checks.${system}.${name};
  app-packaging-api-cross = outputs.checks.${system}."${name}-cross-${target}";
}
