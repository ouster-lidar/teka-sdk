{ pkgs, sdk }:
let
  inherit (pkgs) lib;
  inherit (pkgs.stdenv.hostPlatform) system;
  buildSystem = if pkgs.stdenv.hostPlatform.isDarwin then "aarch64-linux" else system;
  apps = [
    "hello-http"
    "hello-ouster-world"
    "hello-pub"
    "hello-slidev"
    "hello-sub"
    "hello-wayland-gpu"
    "hello-webui"
    "hello-world"
  ];
  roots = [
    sdk.packages.${system}.teka
    sdk.packages.${system}.teka-src
    sdk.packages.${buildSystem}.teka-runner
  ]
  ++ map (app: sdk.teka.apps.${app}.packages.${buildSystem}.aarch64-linux) apps;
  # Capture this flake's derivations: re-evaluating its store copy loses Git metadata.
  installables = map (drv: "${builtins.unsafeDiscardOutputDependency drv.drvPath}^*") roots;
  manifest = pkgs.writeText "cache-root.nix" ''
    { pathsFile, name }:
    builtins.toFile name (builtins.concatStringsSep "\n" (
      map builtins.storePath (builtins.fromJSON (builtins.readFile pathsFile))
    ))
  '';
in
pkgs.writeShellApplication {
  name = "publish-cache";
  runtimeInputs = with pkgs; [
    cachix
    coreutils
    gnugrep
    jq
    nix
  ];
  text = ''
    usage() {
      echo 'Usage: nix run .#publish-cache -- [--dry-run] [PIN]'
      echo 'Push the preferred apps and their build dependencies; optionally pin this checkout.'
    }
    dry_run=false
    if [[ "''${1:-}" == --help ]]; then usage; exit 0; fi
    if [[ "''${1:-}" == --dry-run ]]; then dry_run=true; shift; fi
    if (( $# > 1 )) || [[ "''${1:-}" == -* ]]; then usage >&2; exit 1; fi
    pin="''${1:-}"
    revision=${lib.escapeShellArg (sdk.rev or "")}
    if [[ -n "$pin" && -z "$revision" ]]; then
      echo 'Release pins require a clean, committed checkout.' >&2
      exit 1
    fi
    temporary=$(mktemp -d)
    trap 'rm -rf "$temporary"' EXIT

    nix build --no-update-lock-file --json --out-link "$temporary/apps" \
      ${lib.escapeShellArgs installables} > "$temporary/builds.json"
    mapfile -t derivations < <(jq -r '.[].drvPath' "$temporary/builds.json")
    # Traverse derivations so build-only outputs survive the release pin too.
    nix-store -qR --include-outputs "''${derivations[@]}" \
      | grep -v '\.drv$' | sort -u \
      | jq -Rs 'split("\n") | map(select(length > 0))' > "$temporary/paths.json"
    root=$(TEKA_CACHE_PATHS="$temporary/paths.json" nix eval --impure --raw --file ${manifest} \
      --apply 'f: f { pathsFile = builtins.getEnv "TEKA_CACHE_PATHS"; name = "teka-cache-${system}"; }')
    nix-store --realise "$root" --add-root "$temporary/root" >/dev/null
    echo "Cache root: $root ($(jq length "$temporary/paths.json") paths)"
    if "$dry_run"; then exit 0; fi

    cachix push teka "$root"
    if [[ -n "$pin" ]]; then
      cachix pin teka "$pin-${system}" "$root" --keep-revisions 1
    fi
  '';
}
