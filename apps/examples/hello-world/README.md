# hello-world

A minimal Rust app that prints `hello world`. `default.nix` builds the program and app image; `hello-world.service` starts it.

## Build and deploy

```sh
teka app deploy --build hello-world
```

Verify that `journalctl -u hello-world.service` contains `hello world`.
