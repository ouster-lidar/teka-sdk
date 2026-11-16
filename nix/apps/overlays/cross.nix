final: prev:
(import ./pipewire-cross-no-modemmanager-overlay.nix) final prev
// {
  # ministream 0.99+ (libadwaita → Firefox) fails cross with "Feature introspection
  # cannot be disabled". Upstream adds mesonEmulatorHook in
  # https://github.com/NixOS/nixpkgs/pull/587362 (same pattern as appstream).
  # FIXME: Revisit when the pin includes https://github.com/NixOS/nixpkgs/pull/587362.
  # Upstream uses mesonEmulatorHook when !buildPlatform.canExecute hostPlatform; we rely
  # on postPatch here when binfmt makes canExecute true but Meson cross still fails.
  ministream =
    if prev.stdenv.hostPlatform.system != prev.stdenv.buildPlatform.system then
      prev.ministream.overrideAttrs (old: {
        mesonFlags =
          let
            flags = old.mesonFlags or [ ];
          in
          (builtins.filter (f: !(builtins.isString f && prev.lib.strings.hasInfix "introspection" f)) flags)
          ++ [ "-Dintrospection=auto" ];
        postPatch = (old.postPatch or "") + ''
          substituteInPlace meson.build --replace-fail \
            "required: get_option('introspection').require(meson.can_run_host_binaries())" \
            "required: get_option('introspection').enabled() and meson.can_run_host_binaries()"
        '';
      })
    else
      prev.ministream;
}
