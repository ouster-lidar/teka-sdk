{ lib, ... }:
{
  mode = lib.mkDefault "kiosk";
  startUrl = lib.mkDefault "https://webglreport.com/?v=2";
}
