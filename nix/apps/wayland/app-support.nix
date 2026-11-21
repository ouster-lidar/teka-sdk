{
  name,
  lib,
  stdenv,
  runCommand,
  mesa,
  writeText,
  bash,
  dbus,
  dejavu_fonts,
  linkFarm,
  cacert,
}:

let
  caCertificates = lib.genAttrs [
    "etc/ssl/certs/ca-bundle.crt"
    "etc/ssl/certs/ca-certificates.crt"
    "etc/pki/tls/certs/ca-bundle.crt"
  ] (_: "${cacert}/etc/ssl/certs/ca-bundle.crt");

  hostEtcBindPaths = [
    "-/etc/passwd:/etc/passwd"
    "-/etc/group:/etc/group"
    "-/etc/resolv.conf"
    "-/etc/hosts"
  ]
  ++ map (path: "-/${path}") (lib.attrNames caCertificates)
  ++ [ "-/run/seatd.sock" ];

  nsswitchConf = writeText "teka-portable-nsswitch.conf" ''
    passwd: files
    group: files
    shadow: files
    hosts: files dns
    networks: files
    protocols: files
    services: files
    ethers: files
    rpc: files
    netgroup: files
  '';

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

  # Preload LLVM for the second GLFW context when host Mesa's RUNPATH cannot
  # resolve it. Copy only the shared library to avoid embedding the LLVM package.
  mesaLlvm = runCommand "${name}-libllvm-preload" { } ''
    mkdir -p "$out/lib"
    src=${mesa.llvmPackages.libllvm.lib}/lib
    ver=$(basename "$(readlink -f "$src/libLLVM.so")")
    cp -L "$src/$ver" "$out/lib/"
    ln -sfn "$ver" "$out/lib/libLLVM.so"
  '';

  # Shared PATH / client LD append / wlroots stderr filter for app Sway and
  # Cage+wlroots run loops. Host GPU defaults (WLR_*, LD_LIBRARY_PATH, …) come from
  # `EnvironmentFile=/etc/teka/wlroots.env` on the unit; this snippet only adds
  # closure-specific bits and per-service XDG paths.
  mkShellEnv =
    {
      clientPath,
      clientLd,
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
        (lib.escapeShellArg clientPath)
        (lib.escapeShellArg clientLd)
        "/run/${name}"
        (toString bash)
      ]
      tpl;

  fontsConf = writeText "${name}-fonts.conf" ''
    <?xml version="1.0"?>
    <!DOCTYPE fontconfig SYSTEM "fonts.dtd">
    <fontconfig>
      <dir>${dejavu_fonts}/share/fonts/truetype</dir>
      <cachedir>/run/${name}/fontconfig-cache</cachedir>
    </fontconfig>
  '';

in
{
  inherit
    dbus
    fontsConf
    hostEtcBindPaths
    mkShellEnv
    ;
  mesaLlvmPreload = lib.optionalString stdenv.hostPlatform.isx86_64 ''
    if [[ "$_tegra_l4t" -eq 0 ]]; then
      export LD_PRELOAD="${mesaLlvm}/lib/libLLVM.so''${LD_PRELOAD:+:''$LD_PRELOAD}"
    fi
  '';
  rootPackage = linkFarm "teka-wayland-root" (
    caCertificates
    // {
      "etc/dbus-1/session.conf" = dbusSessionConf;
      "etc/hosts" = writeText "teka-portable-etc-hosts" "127.0.0.1 localhost\n::1 localhost\n";
      "etc/nsswitch.conf" = nsswitchConf;
      "etc/passwd" = builtins.toFile "empty" "";
      "etc/group" = builtins.toFile "empty" "";
      "run/seatd.sock" = seatdSockStub;
    }
  );
}
