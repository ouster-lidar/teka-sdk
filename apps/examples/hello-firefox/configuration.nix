{ lib, ... }:
{
  mode = lib.mkDefault "browser";
  startUrl = lib.mkDefault "https://demos.babylonjs.com/3d-tiles/#lat=37.7749&lon=-122.4194&alt=6000";
}
