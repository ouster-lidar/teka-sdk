# How to Use TekaOS with the Teka CLI

The Teka CLI (teka) finds and signs in to your boxes, builds and deploys your apps, and updates TekaOS. People, CI pipelines, and AI agents all use the same commands.

- **One box or a whole fleet:** Deploy to as many boxes as you like with one command.
- **Verified every time:** Every deploy checks your app's logs to make sure it actually started.
- **Built for automation:** Clean output, no prompts when you pass what it needs, and clear exit codes.

## Install

Install teka from [Set Up a Development Host](02_set-up-a-development-host/00_README.md).

## Quick start

Find your box, select it, then create and deploy an app:

```bash
teka box list --discover
teka box select teka-<serial>
teka app init my-app
cd my-app
teka app deploy --build
```

## Commands

| Command | What it does |
| --- | --- |
| `teka box list` | Lists the boxes you've signed in to before. `*` marks the selected ones. Add `--discover` to search your network. |
| `teka box select [PATTERN…]` | Signs in to boxes and makes them your default deployment targets. |
| `teka box status [PATTERN…]` | Shows whether teka can reach and sign in to each box. |
| `teka box forget [PATTERN…]` | Removes boxes and their saved passwords. |
| `teka app init [DIR]` | Creates a new app in a folder, or in the current folder. Add `--example NAME` to start from a specific starter example. |
| `teka app list` | Lists the apps in the current folder's project. Add `--examples` to list the starter examples. |
| `teka app build [APP…]` | Builds app packages into `./result`. With no name, builds the project's only app. |
| `teka app deploy [APP…]` | Installs apps on your selected boxes. Add `--build` to build them first. Add `--box URL` to pick another box. |
| `teka system status` | Shows the box's TekaOS version and update state. |
| `teka system update FILE` | Installs a TekaOS update. |
| `teka build check` | Checks that your computer can build apps. |
| `teka --help` | Shows every command and option. |

## `teka box`

Use these commands to find your boxes and sign in once.

### list

```bash
teka box list --discover
```

Prints each TekaOS box on your network with its URL, hardware, serial number, part number, TekaOS version, and addresses. Add a pattern to match part of a hostname, part number, or serial number. Without `--discover`, `teka box list` prints the boxes you've signed in to before and marks the selected ones with `*`.

### select

```bash
teka box select teka-<serial>
```

Signs in to every box that matches and makes them your default deployment targets. With no pattern, teka discovers boxes and shows a menu. A full URL, like `http://teka-robot-7.local/`, skips discovery.

teka asks for each box's admin password and saves it in macOS Keychain or the Linux Secret Service. Next time, it reuses the saved password. Selecting several boxes asks once for a shared password; press Enter to type each password instead.

### status and forget

| Command | What it does |
| --- | --- |
| `teka box status` | Prints reachability and sign-in state for your selected boxes. Add `--json` for scripts. |
| `teka box forget [PATTERN…]` | Removes the boxes and their saved passwords. With no pattern, shows a menu. |

## `teka app`

### init

```bash
teka app init my-app
```

Creates `my-app`, pins the Teka SDK commit that supplied it, and stages the files in Git without committing. The folder defaults to `.`. teka starts from hello-world, and the app keeps that ID. See [Starting Your Own App](../02_teka-apps/01_starting-your-own-app.md) for what to do with each file.

To start from another example, add `--example NAME`:

```bash
teka app init my-app --example hello-gpio
```

If the example name is unknown, teka stops before it writes any files. List the starter examples with `teka app list --examples`.

`teka app list`, `teka app build`, and `teka app deploy` read the project in your current folder. teka walks up from there to the nearest `flake.nix` that defines `teka.apps`, and stops at the Git root. Working from another folder? Pass `--flake` with its path.

### list

```bash
teka app list
```

Lists the apps in the current project. From the teka-sdk repository root, it lists every example. Add `--examples` to list the starter examples instead: hello-ouster-world, hello-webui, and hello-world.

### build

```bash
teka app build hello-world
```

Builds app packages and links them in `./result`, one folder per app. teka prints each path. Use this when you want to install the package yourself. With no name, it builds the project's only app.

| Option | What it does |
| --- | --- |
| `--arch` | The CPU to build for: aarch64 (the default, for Jetson devices) or x86_64. |
| `--flake` | The project folder to use, if you're not inside it. |

### deploy

```bash
teka app deploy --build
```

Builds the app for each box, installs it, and checks its logs to make sure it started. Without `--build`, teka installs what's already in `./result`, or a `.tpk` file you name. Run it again to replace an app with a new version. Its data stays.

| Option | What it does |
| --- | --- |
| `--build` | Builds the apps for each box first. |
| `--box` | A box to deploy to. Repeat it for more boxes. |
| `--remove` | Removes the old app before installing, instead of replacing it in place. |
| `--jobs` | How many boxes to deploy to at once. The default is 4. |
| `--profile` | The security profile to install with. The default is trusted. |
| `--flake` | The project folder to use, if you're not inside it. |

teka deploys to every box before it reports problems, so some boxes may update even if others fail.

## `teka system`

Use system commands to check and update TekaOS. They work on one box at a time.

### status

```bash
teka system status
```

Prints the box's status as JSON, including its TekaOS version and the state of its last update.

### update

```bash
teka system update <update>.tpk --wait
```

Sends a TekaOS update package to the box. Learn more in [Update TekaOS on a Box](05_update-tekaos/00_README.md).

| Option | What it does |
| --- | --- |
| `--wait` | Waits until the box restarts on the new version, then prints the final status. If the box rolls back, teka exits with an error that lists the services that failed. |
| `--no-reboot` | Installs the update without restarting. It starts on the next restart. |
| `--box` | The box to update. |

## Which box teka uses

For app deploy and system commands, teka picks boxes in this order:

1. `--box`
2. The TEKA_BOX environment variable
3. Your selected boxes from `teka box select`

App deploy uses every selected box. System commands need exactly one, so pass `--box` when you've selected several. A box can be a hostname, like teka-robot-7.local, or a full URL, like http://teka-robot-7.local/.

## Sign in from scripts and CI

When TEKA_PASSWORD is set, teka uses it instead of the saved password, and never falls back to the saved one. In a script or CI job, set TEKA_PASSWORD and pass `--box`:

```bash
TEKA_PASSWORD='<password>' teka app deploy --build --box teka-<serial>.local
```

teka keeps sessions in memory and never sends one box's password to another.

## Build on a Mac

On a Mac, the installer runs `teka build init`, which sets up a Linux build VM and configures public Nix caches. Learn more in [macOS](02_set-up-a-development-host/01_macos.md).

## Environment variables

| Variable | What it is |
| --- | --- |
| TEKA_PASSWORD | Box password. Overrides the saved password. |
| TEKA_BOX | The box to use when you don't pass `--box` |
| TEKA_APPS_FLAKE_ROOT | The project folder to use when you don't pass `--flake` |
| TEKA_UPDATE_WAIT_SEC | How long system update `--wait` waits, in seconds. The default is 900. |

teka saves known and selected boxes in `~/.config/teka/config.json`.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | It worked. |
| 1 | Something went wrong, like a failed build or an app that didn't start. |
| 2 | The command was wrong, like a missing option or no box to use. |
| 3 | `teka tpk verify` rejected a package. |

teka prints data to stdout and messages to stderr, so you can pipe its output.

## Tab completion

Add this line to ~/.bashrc:

```bash
eval "$(teka completion bash)"
```

For zsh, add the same kind of line to ~/.zshrc:

```bash
eval "$(teka completion zsh)"
```

For fish, run:

```bash
teka completion fish | source
```

Leave the shell name off and teka detects it. Run the line again after you update teka.

## Add your own commands

Any program on your PATH named `teka-\<name>` runs as `teka \<name>`. For example, `teka-hello` runs as `teka hello`. `teka --help` lists the ones you have.

## Coming soon

- Turn a Docker image into a Teka App with one command.
