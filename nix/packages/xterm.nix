{
  fetchurl,
  lib,
  runCommand,
}:
let
  version = "6.0.0";
  fitVersion = "0.11.0";
  xterm = fetchurl {
    url = "https://registry.npmjs.org/@xterm/xterm/-/xterm-${version}.tgz";
    hash = "sha256-kI5m4Er2yNxrAN07VN4Iji6B5e2GYoT9bC+zwtHHo/Y=";
  };
  fit = fetchurl {
    url = "https://registry.npmjs.org/@xterm/addon-fit/-/addon-fit-${fitVersion}.tgz";
    hash = "sha256-JgA7RRehMrZOT/Io/Yil/aP/9eYGx2CT9tz/dy6ezsA=";
  };
in
runCommand "xterm-${version}"
  {
    inherit version;
    meta = {
      description = "Xterm.js browser bundles and Fit Addon";
      homepage = "https://xtermjs.org/";
      license = lib.licenses.mit;
    };
  }
  ''
    web="$out/share/xterm"
    licenses="$out/share/licenses/xterm"
    mkdir -p "$web" "$licenses"
    tar -xOf ${xterm} package/lib/xterm.js > "$web/xterm.js"
    tar -xOf ${xterm} package/css/xterm.css > "$web/xterm.css"
    tar -xOf ${fit} package/lib/addon-fit.js > "$web/xterm-addon-fit.js"
    tar -xOf ${xterm} package/LICENSE > "$licenses/xterm"
    tar -xOf ${fit} package/LICENSE > "$licenses/addon-fit"
  ''
