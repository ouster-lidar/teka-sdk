# Starting Your Own App

Create a new app with `teka app init`, then build and deploy it to your box.

```bash
teka app init my-app
cd my-app
```

teka creates `my-app` from the [hello-world](02_app-examples.md#hello-world) example. The app keeps the hello-world ID until you change it.

This is what you'll see:

| File | What it does |
| --- | --- |
| `flake.nix` | Declares the app and the Teka SDK input. The app ID is the name under `apps.`. |
| `flake.lock` | The SDK commit this app builds with, pinned when you created the app. Commit it with your app. |
| `default.nix` | How the app image builds: the program, its files, and its systemd service. |
| `hello-world.rs` | The program. Other examples use Python, C++, or Rust crates. |
| `hello-world.service` | The systemd unit that starts the program on the box. |
| `README.md` | How to use, build, and deploy the app. |
| `AGENTS.md` | Notes for AI agents working on the app. teka adds it only when the folder doesn't have one. |

`teka app init` also creates a Git repository if the folder isn't in one, and stages the new files. It doesn't commit them.

To start from a different example app, add `--example NAME`. See [App Examples](02_app-examples.md) for more details on starter examples.

## Change the app ID

The app ID appears in three places: the name under `apps.` in `flake.nix`, the `name` in `default.nix`, and the service file name. Change all three together. Use letters, numbers, hyphens, and underscores, up to 64 characters. Don't start it with mc-.

## Where the SDK comes from

`teka app init` pins the Teka SDK to the commit of the checkout or flake that supplied the example. Later builds in the folder use that commit, so the app doesn't need the rest of the repository.

To move to a newer SDK, update the lock from your app's folder:

```bash
nix flake update teka-sdk
```

Then rebuild and check that the app still starts.

## Build and deploy

Select your box once with `teka box select`, if you haven't already. Then list, build, and deploy from your app's folder:

```bash
teka app deploy
```

`teka app deploy` selects packages for each box architecture, installs them, and checks startup logs. Nix downloads cached outputs when available and builds on a cache miss. Your app starts right away and again on every boot. Open **Apps** in the web interface and you'll find it under **Installed apps**.

Change your code, then run the same command again. teka replaces the app and keeps its data.

To deploy to specific boxes instead of your selection, repeat `--box`:

```bash
teka app deploy --box teka-box-1.local --box teka-box-2.local
```

## Next steps

- [Teka Apps](00_README.md): The rules for an app ID and its service.
- [Teka CLI](../03_tekaos/03_teka-cli.md): Every `teka app` command and option.
