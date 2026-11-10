{
  image =
    { ps, pkgs }:
    let
      package =
        pkgs.runCommand "hello-world"
          {
            nativeBuildInputs = [
              pkgs.rustc
              pkgs.stdenv.cc
            ];
            meta.mainProgram = "hello-world";
          }
          ''
            mkdir -p "$out/bin"
            rustc --crate-name hello_world --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} \
              -C linker=${pkgs.stdenv.cc.targetPrefix}cc -O ${./hello-world.rs} -o "$out/bin/hello-world"
          '';
    in
    ps.makeAppImage {
      name = "hello-world";
      unit = pkgs.replaceVars ./hello-world.service { program = pkgs.lib.getExe package; };
      runtimeClosures = [ package ];
    };
}
