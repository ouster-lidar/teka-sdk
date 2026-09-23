{ ps, writeText }:

let
  name = "hello-world";
  jp = ps.hostUnitDefaults {
    hostGpuChecksCfg = {
      enable = false;
    };
  };
in

ps.makeAppImage {
  inherit name;
  unit = writeText "${name}.service" ''
    [Unit]
    Description=hello-world app

    [Service]
    Type=oneshot
    ${jp.serviceFragment}
    ExecStart=/bin/sh -c 'echo hello world'
    SyslogIdentifier=${name}
    StandardOutput=journal
    StandardError=journal
    RemainAfterExit=yes

    [Install]
    WantedBy=jetstream-apps.target
  '';
}
