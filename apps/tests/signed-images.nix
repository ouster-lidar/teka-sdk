{ pkgs, version }:

let
  builder = pkgs.callPackage ../image-builder.nix { inherit version; };
  image = builder.makeAppImage {
    name = "verity-check";
    unit = pkgs.writeText "verity-check.service" "[Service]\nExecStart=/bin/true\n";
  };
  repeat = image.overrideAttrs { name = "verity-check-repeat"; };
in
pkgs.runCommand "signed-app-images-check"
  {
    nativeBuildInputs = with pkgs; [
      cryptsetup
      erofs-utils
      jq
      openssl
      util-linux
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
      tr -d '\000' <part-1 >signature.json
      jq -ej '.rootHash | select(test("^[0-9a-f]{64}$"))' signature.json >rootHash
      test "$(cat rootHash)" = "$(cat "$1"/*.roothash)"
      jq -r .signature signature.json | base64 --decode >signature.der
      openssl cms -verify -inform DER -in signature.der -content rootHash \
        -certfile ${../dev-keys/apps.pem} -nointern -noverify -binary -out /dev/null
      veritysetup verify part-2 part-0 "$(cat rootHash)"
    }
    mkdir a b
    (cd a && check ${image})
    (cd b && check ${repeat})
    cmp a/rootHash b/rootHash
    touch "$out"
  ''
