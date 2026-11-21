{
  appId,
  config,
  lib,
  name,
  pkgs,
  systemdUtils,
  ...
}:
let
  inherit (lib)
    mkIf
    mkMerge
    mkOption
    mkOptionDefault
    types
    ;
  inherit (config) runtime;
  inherit (systemdUtils.unitOptions) commonUnitOptions serviceOptions;
  unitBaseName = lib.removeSuffix ".service" config.unitName;
in
{
  options = {
    program = mkOption {
      type = types.nullOr types.package;
      default = null;
      description = "Package whose main executable supplies the default start command.";
    };
    unitName = mkOption {
      type = types.str;
      readOnly = true;
      default = if name == "main" then "${appId}.service" else "${appId}-${name}.service";
      description = "Generated systemd unit filename.";
    };
    runtime = {
      managed = lib.mkEnableOption "ordering after the Teka manager";
      notify = mkOption {
        type = types.bool;
        default = runtime.managed;
        description = "Whether the service reports readiness with sd_notify.";
      };
      network = mkOption {
        type = types.nullOr (
          types.enum [
            "private"
            "loopback"
            "host"
          ]
        );
        default = if runtime.managed then "loopback" else null;
      };
      gpu = mkOption {
        type = types.nullOr (
          types.enum [
            "cuda"
            "wayland"
          ]
        );
        default = null;
      };
    };
    systemd = mkOption {
      type = types.submoduleWith {
        shorthandOnlyDefinesConfig = true;
        specialArgs.name = unitBaseName;
        modules = [
          (
            commonUnitOptions
            // {
              # These options rely on the host's unit installation machinery.
              options =
                removeAttrs commonUnitOptions.options [
                  "aliases"
                  "enable"
                  "overrideStrategy"
                  "requiredBy"
                  "upheldBy"
                ]
                // {
                  description = commonUnitOptions.options.description // {
                    default = "Teka App ${unitBaseName}";
                  };
                };
            }
          )
          serviceOptions
          systemdUtils.lib.unitConfig
          systemdUtils.lib.stage1ServiceConfig
        ];
      };
      default = { };
    };
  };

  config = mkMerge [
    {
      systemd.unitConfig.X-Teka-PortableGPU = mkIf (runtime.gpu != null) runtime.gpu;
      systemd.serviceConfig.SyslogIdentifier = mkOptionDefault "%N";
    }
    (mkIf (config.program != null) {
      systemd.serviceConfig.ExecStart = mkOptionDefault (lib.getExe config.program);
    })
    (mkIf runtime.managed {
      systemd.after = [ "teka-manager.target" ];
      systemd.wants = [ "teka-manager.target" ];
    })
    (mkIf runtime.notify {
      systemd.serviceConfig = {
        Type = mkOptionDefault "notify";
        NotifyAccess = mkOptionDefault "main";
      };
    })
    (mkIf (runtime.network != null) {
      systemd.environment = {
        LANG = mkOptionDefault "C.UTF-8";
        LC_ALL = mkOptionDefault "C.UTF-8";
      };
      systemd.serviceConfig = {
        PrivateNetwork = runtime.network == "private";
        NetworkNamespacePath = mkIf (runtime.network != "private") "/proc/1/ns/net";
        IPAddressDeny = mkIf (runtime.network == "loopback") "any";
        IPAddressAllow = mkIf (runtime.network == "loopback") "localhost";
      };
    })
    (mkIf (runtime.gpu == "wayland") {
      systemd.serviceConfig = {
        EnvironmentFile = "/etc/teka/wlroots.env";
        ExecStartPre = import ./wayland/host-gpu-checks.nix { inherit (pkgs) writeShellScript; };
      };
    })
  ];
}
