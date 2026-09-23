# Thin `firewall-cmd` for app images: full `firewalld` pulls NetworkManager (and
# cross builds then die in modemmanager/libqmi). Bind `/run/dbus/system_bus_socket` in the
# unit (app images stub that path).
#
# Keep the firewall open/close directly in the unit so `ExecStartPre` and `ExecStopPost`
# stay symmetrical. Non-root `User=` units still need a leading `+` so those commands run
# as root over D-Bus.
{
  lib,
  firewalld,
}:

let
  firewalldCli = firewalld.overrideAttrs (old: {
    buildInputs = lib.filter (p: (lib.getName p) != "networkmanager") old.buildInputs;
  });
  firewallCmd = lib.getExe' firewalldCli "firewall-cmd";
in
{
  inherit firewalldCli firewallCmd;
}
