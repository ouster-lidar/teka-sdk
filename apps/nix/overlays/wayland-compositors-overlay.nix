# App images are Wayland-only compositors (Sway, Cage); do not pull Xwayland into closures.
final: prev:
let
  disableX = {
    enableXWayland = false;
  };
in
{
  wlroots_0_20 = prev.wlroots_0_20.override disableX;
  sway = prev.sway.override disableX;
}
