{
  image =
    { ps, sway }:
    ps.mkFirefoxApp {
      name = "hello-firefox";
      description = "hello-firefox (Sway + Firefox on Wayland)";
      stubId = "hff";
      startUrl = "https://demos.babylonjs.com/3d-tiles/#lat=37.7749&lon=-122.4194&alt=6000";
      compositorPkg = sway;
      startLog = "starting Sway (wlroots) + Firefox on DRM with NVIDIA clients";
    };
}
