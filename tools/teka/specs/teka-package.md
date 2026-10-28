# Teka packages

TPK carries a signed JSON manifest and an ordered set of payloads. Each payload is one zstd frame. Bao authenticates the stored bytes and supports streaming and disk access.

## Container

```text
TPK
|
+-- Prefix (16 bytes)
|   +-- Magic: "TPK 1\n\0\0"
|   +-- CMS length (u32 little-endian)
|   `-- Reserved zero (u32 little-endian)
|
+-- CMS SignedData
|   +-- Manifest JSON (eContent)
|   |   +-- format, arch, bao_root
|   |   `-- components and their payload descriptors
|   +-- Signing certificate
|   `-- Signature
|
`-- Bao encoding
    `-- Concatenated stored payloads, for example:
        +-- zstd(os.raw)
        +-- zstd(os.efi)
        +-- zstd(app-a.raw)
        `-- zstd(app-b.raw)
```

The CMS object and embedded manifest are each 1–65536 bytes. Check the CMS length before allocating its buffer and reject nonzero reserved bytes. Bao starts immediately at byte `16 + cms_length`, without additional padding.

The manifest authenticates payload identities, order and stored lengths. Its Bao root authenticates the stored bytes at their declared positions. Payloads have no separate headers or padding.

### Bao layout

| Property | Value |
|----------|-------|
| Root | BLAKE3 of the concatenated stored payloads |
| Group size | 65536 bytes |
| Encoding | Full-range combined preorder |
| Input length | Checked sum of all payload `size` values |
| Size prefix | None; length comes from the signed manifest |

Require the encoded length to match this geometry. Reject truncation and trailing encoded bytes.

Payload order follows component order: image first, then UKI when present. A payload's logical offset is the checked sum of preceding stored lengths.

Bao proofs are interleaved with data groups in the encoded file. The Bao seek reader maps logical payload offsets to physical file positions. Groups can cross payload boundaries.

## Manifest

| Field | Meaning |
|-------|---------|
| `format` | `1` |
| `arch` | `aarch64` or `x86_64` |
| `bao_root` | Bao root as 64 lowercase hexadecimal digits |
| `components` | Nonempty, ordered component list |

### Components

| Field | Meaning |
|-------|---------|
| `type` | `app` or `os` |
| `id` | Nonblank component identity |
| `version` | Nonblank component version |
| `root_hash` | Native verity root as 64 lowercase hexadecimal digits |
| `image` | Image payload descriptor |
| `uki` | Required UKI payload descriptor for OS components |

An app image is named `<id>.raw` and has no UKI. App IDs contain at most 64 ASCII bytes. They start with an alphanumeric character and otherwise use letters, digits, `-` or `_`.

An OS component names its image and required UKI through their payload descriptors; the names are independent of the component ID. The package format can contain an OS component and multiple apps. Each component carries its own version, allowing the OS and apps to advance independently.

### Payload descriptors

| Field | Meaning |
|-------|---------|
| `path` | Unique, flat logical name |
| `size` | Stored zstd frame byte count |

Names are at most 100 UTF-8 bytes. Names contain neither `/`, `\` nor NUL. `.` and `..` are invalid names. They identify payloads, rather than filesystem extraction paths.

| Limit | Value |
|-------|-------|
| Stored or decoded payload size | 1–8589934591 bytes |
| Total stored payload size | At most 64 GiB |

- Reject unknown or duplicate JSON fields and duplicate payload names.
- Reject missing fields, unsupported values and overflowing size calculations.
- Require finalized descriptors to cover the complete Bao input with contiguous ranges.

### Pack drafts

Draft manifests supply component metadata and payload paths. They may omit `bao_root` and payload `size`.

`package pack` stages the payloads and fills those fields before signing. It replaces supplied storage values with the actual values.

## Manifest signatures

| Property | Required profile |
|----------|------------------|
| Container | DER CMS ContentInfo carrying attached SignedData |
| Content type | `id-data` |
| Signed content | Exact manifest JSON bytes in `eContent` |
| Algorithm | SHA-256 with RSA PKCS#1 v1.5 |
| Key | RSA-3072 |
| Certificate | One embedded signing certificate |
| Signers | Exactly one |
| Attributes | No signed or unsigned attributes |
| Versions | SignedData 1 and SignerInfo 1 |

Require definite-length, minimally encoded DER and embedded content. Reject detached manifests, CRLs, unsupported profiles and trailing bytes.

### Publisher trust

Trusted verification pins the signer to a configured certificate:

| Package contents | Certificate bundle |
|------------------|--------------------|
| Apps only | `apps.pem` |
| Any OS component | `os.pem` |

Load configured certificates before parsing manifest JSON. Verify the signature first, then parse the manifest and enforce its signing role.

Offline inspection can verify a signature with its embedded certificate. Without configured certificates, signature validity does not establish publisher trust; report publisher trust as not checked.

## Payload decoding

- Each payload is one ordinary zstd frame with a known, nonzero content size.
- Read the decoded size from the authenticated frame header and enforce the payload size limit before decoding.
- Limit the decoder window to 8 MiB.
- Require exact stored-byte consumption, frame completion and the declared content size.
- Reject additional frames or trailing data within a payload.

`package pack` supports zstd levels 1–19, with level 9 as the default. Every payload stays zstd-encoded, including when the frame is larger than its input. The packer omits zstd checksums; BLAKE3 authenticates the frame and its contents.

## Reading a package

1. Read the prefix and check metadata lengths.
2. Verify the manifest signature, then parse and validate its descriptors.
3. Check the signing role, size bounds and derived Bao length.
4. Verify each accessed proof and complete Bao group before decoding its bytes.

An authentication or decoder error permanently fails that reader. Reopened payload reads revalidate their Bao groups.

### HTTP streaming

1. Read the Bao stream sequentially, with HTTP content decompression disabled.
2. Drain unselected stored payloads without decoding them.
3. Decode the selected payload and emit the requested logical range.
4. Consume the complete Bao encoding and require transport EOF.

Output may precede a later error. Check command success before using the complete result.

### Disk access

1. Calculate the selected payload's logical offset from signed stored lengths.
2. Read and verify its Bao proof path and data groups.
3. Decode that payload; earlier payloads need no traversal or decompression.

A compressed range requires decoding from that payload's beginning. `package stream` drains the selected payload to complete its decoder checks. Other payloads remain unchecked. A lower-level read stopped early authenticates only the groups it accessed.

## Native verification

`package verify` also checks native payload signatures and verity data.

DDIs contain GPT root, root-verity and root-verity-sig partitions. Validate discoverable partition types, architecture, bounds and geometry. The 16 KiB signature partition contains NUL-padded JSON:

| Field | Encoding |
|-------|----------|
| `rootHash` | 64 lowercase hexadecimal digits |
| `certificateFingerprint` | Lowercase SHA-256 of the signer certificate DER |
| `signature` | Standard-base64 DER CMS |

Native signatures use detached CMS over the lowercase hexadecimal root hash, with the same RSA/SHA-256 algorithm and no attributes.

1. Verify the native signature against the component's `apps.pem` or `os.pem` trust role.
2. Require its signed hash to equal the component's `root_hash`.
3. Require dm-verity format 1 with SHA-256, then recompute and compare the stored tree and root.

OS verification also checks the UKI signature and architecture, with these signed bindings:

- `roothash=` matches the image root.
- `IMAGE_ID` matches the component ID.
- `IMAGE_VERSION + "+" + BUILD_ID` matches the component version.

| Verification limit | Value |
|--------------------|-------|
| Verity partition | At most 64 MiB |
| UKI | At most 256 MiB |

See [CLI contracts](teka-cli.md) for commands, arguments and output fields.
