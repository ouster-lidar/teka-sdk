{ sdk, system }:

let
  pkgs = sdk.inputs.app-nixpkgs.legacyPackages.${system};
  inherit (pkgs) lib;
  templates = builtins.removeAttrs sdk.templates [ "default" ];
  template = pkgs.writeTextDir "flake.nix" ''
    {
      outputs = _: {
        templates = builtins.fromJSON ${lib.generators.toPretty { } (builtins.toJSON templates)};
      };
    }
  '';
in
pkgs.runCommand "example-templates-check" { nativeBuildInputs = [ pkgs.nix ]; } ''
  export HOME="$TMPDIR/home"
  mkdir -p "$HOME"
  ${lib.concatMapAttrsStringSep "\n" (name: example: ''
    mkdir ${lib.escapeShellArg name}
    (
      cd ${lib.escapeShellArg name}
      nix --extra-experimental-features 'nix-command flakes' --store "$TMPDIR/store" \
        flake init --offline --impure --template ${lib.escapeShellArg "${template}#${name}"} > "$TMPDIR/init.log" 2>&1 \
        || { cat "$TMPDIR/init.log"; exit 1; }
      diff -r ${lib.escapeShellArg "${example.path}"} .
    )
  '') templates}
  touch "$out"
''
