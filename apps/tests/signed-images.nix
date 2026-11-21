{ pkgs, sdk }:

let
  name = "packaging-probe";
  inherit (pkgs.stdenv.hostPlatform) system;
  outputs = sdk.lib.mkAppOutputs {
    self.dirtyShortRev = "";
    systems = [ system ];
    apps = [
      {
        package = ./fixtures/app.nix;
        modules = [ { services.main.systemd.serviceConfig.ExecStart = "/bin/true"; } ];
      }
    ];
  };
  package = outputs.packages.${system}.${name}.targets.${system};
  inherit (package) image tpkFileName;
  version = "1.2.3-rc.1+19700101000000.g0000000.ne3b0c44";
  repeat = image.overrideAttrs { name = "verity-check-repeat"; };
in
assert tpkFileName == "${name}-${system}-v${version}.tpk";
pkgs.runCommand "signed-app-images-check"
  {
    nativeBuildInputs = with pkgs; [
      cryptsetup
      erofs-utils
      jq
      openssl
      util-linux
      sdk.packages.${system}.teka
    ];
  }
  ''
    check() {
      raw=("$1"/*.raw)
      sfdisk --json "$raw" >partitions.json
      jq -e '.partitiontable.partitions | length == 3' partitions.json
      for p in 0 1 2; do
        read -r offset size < <(jq -r --argjson p "$p" '.partitiontable | [.partitions[$p].start * .sectorsize, .partitions[$p].size * .sectorsize] | @tsv' partitions.json)
        dd if="$raw" of="part-$p" iflag=skip_bytes,count_bytes skip="$offset" count="$size" status=none
      done
      test "$(stat -c %s part-1)" = 16384
      fsck.erofs part-2
      dump.erofs -s part-2 | grep -E '^Filesystem incompatible features:.*big_pcluster.*fragments'
      tr -d '\000' <part-1 >signature.json
      jq -ej '.rootHash | select(test("^[0-9a-f]{64}$"))' signature.json >rootHash
      test "$(cat rootHash)" = "$(cat "$1"/*.roothash)"
      jq -r .signature signature.json | base64 --decode >signature.der
      openssl cms -verify -inform DER -in signature.der -content rootHash \
        -certfile ${../../nix/apps/dev-keys/apps.pem} -nointern -noverify -binary -out /dev/null
      veritysetup verify part-2 part-0 "$(cat rootHash)"
    }
    mkdir a b
    (cd a && check ${image})
    (cd b && check ${repeat})
    cmp a/rootHash b/rootHash
    teka tpk inspect --json "${package}/${tpkFileName}" |
      jq -e --arg version ${pkgs.lib.escapeShellArg version} '.manifest.components[0].version == $version'
    touch "$out"
  ''
