# hello-world

A minimal Rust app that prints `hello world`. `package.nix` builds the program and declares its service.

## Build and deploy

Run this command from the app directory.

```sh
teka app deploy
```

Verify that `journalctl -u hello-world.service` contains `hello world`.
