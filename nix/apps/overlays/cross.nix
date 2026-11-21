_: prev:
let
  inherit (prev.stdenv) buildPlatform hostPlatform;
in
{
  # Libtool tries to run the target ldconfig when installing modules.
  libcanberra-gtk3 =
    if buildPlatform == hostPlatform then
      prev.libcanberra-gtk3
    else
      prev.libcanberra-gtk3.overrideAttrs (old: {
        postPatch = (old.postPatch or "") + ''
          substituteInPlace configure --replace-fail 'ldconfig -n $libdir' ':'
        '';
      });
}
