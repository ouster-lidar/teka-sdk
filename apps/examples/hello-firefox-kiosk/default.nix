{
  image =
    { ps, cage }:
    ps.mkFirefoxApp {
      name = "hello-firefox-kiosk";
      description = "hello-firefox-kiosk (Cage + Firefox kiosk on Wayland)";
      stubId = "hfk";
      startUrl = "https://webglreport.com/?v=2";
      compositorPkg = cage;
      kiosk = true;
      startLog = "starting Cage (wlroots) + Firefox kiosk on DRM with NVIDIA clients";
    };
}
