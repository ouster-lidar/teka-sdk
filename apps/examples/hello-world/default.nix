{ ps, writeText }:

let
  name = "hello-world";
  jp = ps.jetstreamPortableService {
    hostGpuChecksCfg = {
      enable = false;
    };
  };
in

ps.makePortableErofsImage {
  inherit name;
  unit = writeText "${name}.service" ''
    [Unit]
    Description=hello-world portable

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
