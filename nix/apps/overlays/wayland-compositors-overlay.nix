# App images are Wayland-only compositors (Sway, Cage); do not pull Xwayland into closures.
final: prev:
let
  disableX = {
    enableXWayland = false;
  };
in
{
  # App launchers start a private session bus without X11 or systemd activation.
  dbus-session = prev.dbus.override {
    enableSystemd = false;
    x11Support = false;
  };
  wlroots_0_20 = prev.wlroots_0_20.override disableX;
  sway = prev.sway.override (disableX // { dbus = final.dbus-session; });
}
