{ lib }:
{
  rootPartition,
  image,
  architecture,
  key,
  certificate,
}:
''
  mkdir -p "$out" repart.d
  cat >repart.d/30-root.conf <<EOF
  [Partition]
  Type=root
  Verity=data
  VerityMatchKey=root
  ${rootPartition}
  EOF
  cat >repart.d/10-root-verity.conf <<EOF
  [Partition]
  Type=root-verity
  Verity=hash
  VerityMatchKey=root
  Minimize=best
  EOF
  cat >repart.d/20-root-verity-sig.conf <<EOF
  [Partition]
  Type=root-verity-sig
  Verity=signature
  VerityMatchKey=root
  EOF
  install -m600 ${key} signing.key
  unshare --map-root-user fakeroot systemd-repart --offline=yes --empty=create --size=auto \
    --seed=0d1fe1f5-0000-4000-8000-000000000000 --architecture=${architecture} \
    --definitions=repart.d --private-key=signing.key \
    --certificate=${certificate} --json=short "$out/${image}" >repart.json
  jq -er '.[] | select(.type == "root-x86-64" or .type == "root-arm64") | .roothash | select(test("^[0-9a-f]{64}$"))' repart.json \
    >"$out/${lib.removeSuffix ".raw" image}.roothash"
''
