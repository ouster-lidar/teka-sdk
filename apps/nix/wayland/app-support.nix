{
  lib,
  writeText,
  version,
  caBundleCopyFiles,
  etcHostsStub,
  nsswitchConf,
  hostEtcBindPaths,
  passwdGroupBindPaths,
}:

let
  dbusSessionConf = writeText "teka-portable-dbus-session.conf" ''
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <type>session</type>
      <keep_umask/>
      <listen>unix:tmpdir=/tmp</listen>
      <auth>EXTERNAL</auth>
      <standard_session_servicedirs/>
      <policy context="default">
        <allow send_destination="*" eavesdrop="true"/>
        <allow eavesdrop="true"/>
        <allow own="*"/>
      </policy>
      <includedir>session.d</includedir>
      <includedir>/etc/dbus-1/session.d</includedir>
      <limit name="max_incoming_bytes">1000000000</limit>
      <limit name="max_incoming_unix_fds">250000000</limit>
      <limit name="max_outgoing_bytes">1000000000</limit>
      <limit name="max_outgoing_unix_fds">250000000</limit>
      <limit name="max_message_size">1000000000</limit>
      <limit name="service_start_timeout">120000</limit>
      <limit name="auth_timeout">240000</limit>
      <limit name="pending_fd_timeout">150000</limit>
      <limit name="max_completed_connections">100000</limit>
      <limit name="max_incomplete_connections">10000</limit>
      <limit name="max_connections_per_user">100000</limit>
      <limit name="max_pending_service_starts">10000</limit>
      <limit name="max_names_per_connection">50000</limit>
      <limit name="max_match_rules_per_connection">50000</limit>
      <limit name="max_replies_per_connection">50000</limit>
    </busconfig>
  '';

  seatdSockStub = builtins.toFile "teka-portable-seatd.sock" "";

  # Shared PATH / client LD append / wlroots stderr filter for app Sway and
  # Cage+wlroots run loops. Host GPU defaults (WLR_*, LD_LIBRARY_PATH, …) come from
  # `EnvironmentFile=/etc/teka/wlroots.env` on the unit; this snippet only adds
  # closure-specific bits and per-service XDG paths.
  wlrootsSwayEnvShSubst =
    {
      clientPath,
      clientLd,
      xdgRuntimeDir,
      bash,
    }:
    let
      tpl = builtins.readFile ./wlroots-sway-env.sh;
    in
    lib.replaceStrings
      [
        "@clientPath@"
        "@clientLd@"
        "@xdgRuntimeDir@"
        "@bash@"
      ]
      [
        clientPath
        clientLd
        xdgRuntimeDir
        bash
      ]
      tpl;

  mkFontconfigConf =
    {
      name,
      dejavu_fonts,
      cacheDir,
    }:
    writeText name ''
      <?xml version="1.0"?>
      <!DOCTYPE fontconfig SYSTEM "fonts.dtd">
      <fontconfig>
        <dir>${dejavu_fonts}/share/fonts/truetype</dir>
        <cachedir>${cacheDir}</cachedir>
      </fontconfig>
    '';

  waylandAppBaseCopyFiles = [
    {
      path = "etc/dbus-1/session.conf";
      source = dbusSessionConf;
    }
    {
      path = "etc/hosts";
      source = etcHostsStub;
    }
    {
      path = "etc/nsswitch.conf";
      source = nsswitchConf;
    }
  ]
  ++ caBundleCopyFiles
  ++ [
    {
      path = "run/seatd.sock";
      source = seatdSockStub;
    }
  ];

  appGpuManifestJson =
    {
      id,
      gpu,
    }:
    writeText "app-manifest.json" ''
      {"id":"${id}","version":"${version.imageVersion}","portable":{"displayService":true,"gpu":"${gpu}"}}
    '';

  appGpuManifestCopyFiles =
    {
      name,
      gpu,
    }:
    [
      {
        path = "usr/lib/teka/mission-control/manifest.json";
        source = appGpuManifestJson {
          id = name;
          inherit gpu;
        };
      }
    ];

  mkGpuWaylandUnit =
    {
      name,
      description,
      execStart,
      jp,
      runtimeDirectory,
      useTty ? false,
      serviceType ? "simple",
      bindHostEtc ? true,
      extraUnit ? "",
      extraService ? "",
      user ? "root",
    }:
    writeText "${name}.service" ''
      [Unit]
      Description=${description}
      X-Teka-PortableGPU=wayland
      After=systemd-user-sessions.service network.target network-online.target time-sync.target seatd.service
      Wants=network-online.target time-sync.target seatd.service
      Conflicts=getty@tty1.service display-manager.service
      ${extraUnit}

      [Service]
      Type=${serviceType}
      User=${user}
      ${jp.serviceFragment}
      ExecStart=${execStart}
      SyslogIdentifier=${name}
      Restart=always
      RestartSec=3
      ${lib.optionalString useTty ''
        StandardInput=tty
        StandardOutput=journal
        TTYPath=/dev/tty1
        TTYReset=true
        TTYVHangup=true
        TTYVTDisallocate=true
      ''}
      ${lib.optionalString bindHostEtc hostEtcBindPaths}
      ${lib.optionalString (!bindHostEtc) passwdGroupBindPaths}
      BindReadOnlyPaths=-/run/seatd.sock
      RuntimeDirectory=${runtimeDirectory}
      RuntimeDirectoryMode=0700
      ${extraService}

      [Install]
      WantedBy=teka-apps.target
    '';

in
{
  inherit
    dbusSessionConf
    seatdSockStub
    wlrootsSwayEnvShSubst
    mkFontconfigConf
    waylandAppBaseCopyFiles
    appGpuManifestCopyFiles
    mkGpuWaylandUnit
    ;
}
