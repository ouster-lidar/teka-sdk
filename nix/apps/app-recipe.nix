{
  pkgs,
  teka,
  modules,
  buildImage,
}:
recipe:
let
  inherit (pkgs) lib;
  inherit (teka) appId;
  inherit (lib) mkOption types;
  systemdUtils = {
    lib = import (pkgs.path + "/nixos/lib/systemd-lib.nix") {
      inherit lib pkgs;
      config.systemd = {
        enableStrictShellChecks = false;
        globalEnvironment = { };
      };
      utils = { };
    };
    unitOptions = import (pkgs.path + "/nixos/lib/systemd-unit-options.nix") {
      inherit lib systemdUtils;
    };
  };

  options = {
    services = mkOption {
      type = types.attrsOf (
        types.submoduleWith {
          modules = [
            ./service.nix
            { systemd.environment.APP_ID = lib.mkDefault appId; }
          ];
          specialArgs = { inherit appId pkgs systemdUtils; };
        }
      );
    };
    image = {
      packages = mkOption {
        type = types.listOf types.package;
        default = [ ];
        description = "Package outputs to copy into the image root.";
      };
    };
  };
  config =
    (lib.evalModules {
      class = "tekaApp";
      specialArgs = { inherit pkgs teka; };
      modules = [
        {
          inherit options;
          config = {
            services.main.systemd.wantedBy = lib.mkAfter [ "teka-apps.target" ];
            image.packages = lib.mkIf (displayCount > 0) (lib.mkBefore [ teka.lib.wayland.rootPackage ]);
          };
        }
        recipe
      ]
      ++ modules;
    }).config;
  inherit (config) image services;
  displayCount = lib.count (service: service.runtime.gpu == "wayland") (lib.attrValues services);
  renderService =
    role: service:
    let
      inherit (service) unitName;
      inherit (service.systemd) serviceConfig;
      # An empty assignment resets the command list.
      execStart = lib.last ([ "" ] ++ lib.toList (serviceConfig.ExecStart or [ ]));
      unit = systemdUtils.lib.serviceToUnit service.systemd;
    in
    assert lib.assertMsg (
      builtins.stringLength unitName <= 255
    ) "Generated service filename for role '${role}' exceeds 255 bytes";
    assert lib.assertMsg (unit.name == unitName) "Expected generated service name ${unitName}";
    assert lib.assertMsg (toString execStart != "") "Expected ExecStart= for ${unitName}";
    lib.nameValuePair unitName (pkgs.writeText unitName unit.text);
  units = lib.mapAttrs' renderService services;
in
assert lib.assertMsg (lib.all (role: builtins.match "[a-z0-9_-]+" role != null) (
  lib.attrNames services
)) "Service roles must contain only lowercase letters, digits, underscores, and hyphens";
assert lib.assertMsg (displayCount <= 1) "Expected at most one display service";
buildImage {
  inherit units;
  inherit (image) packages;
}
