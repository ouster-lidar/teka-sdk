# Layout

Apps and build/deploy commands: [README](../README.md).

## Tree

- `apps/examples/hello-*` — demos listed in the README
- `apps/nix/` — portable builder and shared packages
- `apps/flake.nix` — registers those apps
- `apps/specs/` — `cross-build.md`, `erofs-ddi.md`, `frameset-socket.md`
- `apps/scripts/jsd-app-deploy.sh` and `apps/scripts/lib/flake-root.sh`

hello-ouster-world stores OSF recordings under `/srv/recordings/` (host bind).
It can optionally open a FrameSet socket
from **Ouster Dynamo** (proprietary app, installed separately) on
`/run/ouster-dynamo/frameset.sock`.
