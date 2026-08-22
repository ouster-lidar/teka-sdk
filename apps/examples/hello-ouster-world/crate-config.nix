{ pkgs, ... }:

let
  pkgCfg = [ pkgs.pkg-config ];
  gstInputs = with pkgs; [
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
  ];
  gstSys = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ pkgCfg;
    buildInputs = (attrs.buildInputs or [ ]) ++ gstInputs;
  };
in
{
  gstreamer-sys = gstSys;
  gstreamer-base-sys = gstSys;
  gstreamer-app-sys = gstSys;
  glib-sys = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ pkgCfg;
    buildInputs = (attrs.buildInputs or [ ]) ++ [ pkgs.glib ];
  };
  gio-sys = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ pkgCfg;
    buildInputs = (attrs.buildInputs or [ ]) ++ [ pkgs.glib ];
  };
  gobject-sys = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ pkgCfg;
    buildInputs = (attrs.buildInputs or [ ]) ++ [ pkgs.glib ];
  };
  wayland-sys = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ pkgCfg;
    buildInputs = (attrs.buildInputs or [ ]) ++ [ pkgs.wayland ];
  };
  wayland-backend = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ pkgCfg;
    buildInputs = (attrs.buildInputs or [ ]) ++ [ pkgs.wayland ];
  };
}
