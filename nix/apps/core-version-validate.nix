# SemVer 2.0 MAJOR.MINOR.PATCH with optional dot-separated pre-release identifiers.
# Keep aligned with appliance image version validation when the scheme changes.
{ lib }:

coreRaw:
let
  versionBase = lib.head (lib.splitString "-" coreRaw);
  prereleasePart =
    let
      segments = lib.splitString "-" coreRaw;
    in
    if lib.length segments > 1 then lib.concatStringsSep "-" (lib.drop 1 segments) else null;

  numericIdOk = id: id == "0" || !(lib.hasPrefix "0" id);

  coreSegmentOk = seg: builtins.match "^[0-9]+$" seg != null && numericIdOk seg;

  identifierOk =
    id:
    id != ""
    && (
      if builtins.match "^[0-9]+$" id != null then
        numericIdOk id
      else
        builtins.match "^[0-9A-Za-z-]+$" id != null
    );

  prereleaseOk = part: part != null && part != "" && lib.all identifierOk (lib.splitString "." part);

  coreSegments = lib.splitString "." versionBase;
in
if lib.hasInfix "+" coreRaw then
  throw "apps image version: malformed core '${coreRaw}' (build metadata not allowed in IMAGE_VERSION)"
else if lib.length coreSegments != 3 || !lib.all coreSegmentOk coreSegments then
  throw "apps image version: malformed core '${coreRaw}' (expected MAJOR.MINOR.PATCH[-prerelease])"
else if prereleasePart == null then
  coreRaw
else if !prereleaseOk prereleasePart then
  throw "apps image version: invalid SemVer pre-release in '${coreRaw}'"
else
  coreRaw
