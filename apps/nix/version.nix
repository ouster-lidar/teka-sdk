# Compose for distributable filenames (`<id>-<system>-v${imageVersion}+${buildId}.raw`).
# Portable os-release IMAGE_VERSION / VERSION / BUILD_ID follow the app release scheme;
# VERSION_ID is GPU compatibility generation 1 (not SemVer).
# Path flakes (`hello-portable` via path:../apps) have no git rev; `rev` is then `0000000`.
{ self, lib }:

let
  validateImageCore = import ./core-version-validate.nix { inherit lib; };

  imageVersion =
    let
      t = builtins.getEnv "JETSTREAM_TEST_VERSION";
      raw = if t != "" then t else "0.1.0-alpha.1";
    in
    validateImageCore raw;

  rev =
    let
      sr = self.shortRev or "";
      dirty = builtins.replaceStrings [ "-dirty" ] [ "" ] (self.dirtyShortRev or "");
    in
    if sr != "" && sr != "0000000" then
      sr
    else if dirty != "" then
      dirty
    else
      "0000000";

  buildId =
    let
      id = "${self.lastModifiedDate or "19700101000000"}.g${rev}.n${
        builtins.substring 0 7 (builtins.hashString "sha256" (self.narHash or ""))
      }";
    in
    if builtins.match "^[0-9]{14}\\.g[0-9a-f]+\\.n[0-9a-f]{7}$" id == null then
      throw "apps image version: malformed BUILD_ID '${id}'"
    else
      id;

  compose = "${imageVersion}+${buildId}";
in
{
  inherit imageVersion buildId compose;
  appArtifactRawName = id: system: "${id}-${system}-v${compose}.raw";
}
