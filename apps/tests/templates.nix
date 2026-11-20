{ sdk, system }:

let
  pkgs = sdk.inputs.nixpkgs.legacyPackages.${system};
  inherit (pkgs) lib;
  templates = builtins.removeAttrs sdk.templates [ "default" ];
in
pkgs.runCommand "example-templates-check"
  {
    nativeBuildInputs = [
      pkgs.gitMinimal
      pkgs.jq
      pkgs.nix
      sdk.packages.${system}.teka
    ];
  }
  ''
    export HOME="$TMPDIR/home"
    export NIX_CONFIG="
      extra-experimental-features = nix-command flakes
      store = $TMPDIR/store
      substituters =
      flake-registry =
    "
    # Use separate Git sources for the project catalog and its templates' SDK input.
    provider="$TMPDIR/provider"
    sdkSource="$TMPDIR/sdk"
    mkdir -p "$HOME" "$provider/examples" "$sdkSource"
    echo '{ outputs = _: { }; }' > "$sdkSource/flake.nix"
    git init -q "$sdkSource"
    git -C "$sdkSource" add flake.nix
    git -C "$sdkSource" -c user.name=Teka -c user.email=teka@example.invalid commit -qm sdk
    sdkRev=$(git -C "$sdkSource" rev-parse HEAD)
    cat > "$provider/flake.nix" <<'EOF'
    {
      outputs = _: {
        templates = builtins.mapAttrs (name: _: {
          path = ./examples + ("/" + name);
          description = name;
        }) (builtins.readDir ./examples);
      };
    }
    EOF
    ${lib.concatMapAttrsStringSep "\n" (name: example: ''
      cp -r ${lib.escapeShellArg "${example.path}"} "$provider/examples"/${lib.escapeShellArg name}
    '') templates}
    for source in "$provider/examples/"*; do
      chmod u+w "$source"
      substituteInPlace "$source/flake.nix" --replace-fail \
        'github:ouster-lidar/teka-sdk/main' "git+file://$sdkSource?rev=$sdkRev"
    done
    git init -q "$provider"
    git -C "$provider" add .
    git -C "$provider" -c user.name=Teka -c user.email=teka@example.invalid commit -qm templates
    revision=$(git -C "$provider" rev-parse HEAD)
    for source in "$provider/examples/"*; do
      name="''${source##*/}"
      mkdir "nix-$name"
      (
        cd "nix-$name"
        nix flake init --offline --template "git+file://$provider?rev=$revision#$name"
      )
      diff -r "$source" "nix-$name"

      teka app init --flake "$provider" --example "$name" "teka-$name"
      diff -r -x .git -x AGENTS.md -x flake.lock "nix-$name" "teka-$name"
      git -C "teka-$name" ls-files --error-unmatch AGENTS.md flake.lock
      test -z "$(git -C "teka-$name" ls-files --others)"
      jq -e --arg rev "$sdkRev" '
        .nodes as $nodes | $nodes[$nodes[.root].inputs["teka-sdk"]].locked.rev == $rev
      ' "teka-$name/flake.lock" >/dev/null
    done
    touch "$out"
  ''
