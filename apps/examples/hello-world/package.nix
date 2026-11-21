{
  lib,
  pkgs,
  teka,
}:
teka.lib.mkAppImage {
  services.main = {
    program =
      pkgs.runCommandCC "hello-world"
        {
          nativeBuildInputs = [ pkgs.rustc ];
          meta.mainProgram = "hello-world";
        }
        ''
          mkdir -p "$out/bin"
          rustc --crate-name hello_world --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget} \
            -C linker="$CC" -O ${./hello-world.rs} -o "$out/bin/hello-world"
        '';
    systemd.serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = lib.mkDefault true;
    };
  };
}
