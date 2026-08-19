# Back-compat wrapper; prefer nix/open-webui-docker.nix.
{ ... }@args:
import ./open-webui-docker.nix (
  args
  // {
    dockerArch = "arm64";
  }
)
