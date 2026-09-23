# Cross crate2nix: use buildRustPackages.rustc for the cross host
# (pkgs.rustc bootstrap is broken on nixpkgs 1.95).
{
  lib,
  pkgs,
  crateOverridesForPkgs,
}:

let
  isCross = pkgs.stdenv.hostPlatform != pkgs.stdenv.buildPlatform;
  stableRust = pkgs.rust.packages.stable.buildRustPackages;
  crossRust = stableRust.rustc;

  buildRustCrateForPkgs =
    targetPkgs:
    if !isCross || targetPkgs.stdenv.hostPlatform != pkgs.stdenv.hostPlatform then
      targetPkgs.buildRustCrate
    else
      let
        buildCrate = (import "${pkgs.path}/pkgs/build-support/rust/build-rust-crate") {
          inherit (targetPkgs) stdenv;
          inherit (pkgs)
            lib
            fetchCrate
            pkgsBuildBuild
            jq
            libiconv
            ;
          defaultCrateOverrides = crateOverridesForPkgs targetPkgs;
          rustc = crossRust;
          cargo = stableRust.cargo;
        };
      in
      crate:
      buildCrate (
        crate
        // {
          rust = crossRust;
          preBuild = (crate.preBuild or "") + ''
            export PATH="${crossRust}/bin''${PATH:+:$PATH}"
          '';
        }
      );
in
{
  cargoNix,
  packageId,
  rootFeatures ? [ "default" ],
}:
let
  cargoNixPkg = pkgs.callPackage cargoNix (
    {
      defaultCrateOverrides = crateOverridesForPkgs pkgs;
      inherit rootFeatures;
    }
    // lib.optionalAttrs isCross {
      inherit buildRustCrateForPkgs;
    }
  );
in
if isCross then
  cargoNixPkg.internal.buildRustCrateWithFeatures {
    inherit packageId;
    features = rootFeatures;
    buildRustCrateForPkgsFunc = buildRustCrateForPkgs;
  }
else if cargoNixPkg ? rootCrate then
  cargoNixPkg.rootCrate.build
else
  cargoNixPkg.workspaceMembers.${packageId}.build
