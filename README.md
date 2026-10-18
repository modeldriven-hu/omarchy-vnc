# Omarchy VNC

Omarchy VNC is a Rust VNC client for Omarchy. It connects to Macs using Apple
Remote Desktop account authentication as well as ordinary VNC servers. Choose
one of two viewers:

- `omarchy-vnc-egui` — the default OpenGL viewer; it does not require GTK.
- `omarchy-vnc-gtk` — a GTK4 viewer.

Both start with a terminal connection manager and share saved profiles. You can
also start the original graphical connection screen with `--gui`.

> The login exchange for Apple Remote Desktop is protected, but the remote
> desktop and input traffic are not encrypted. Connect over a trusted network
> or a separately secured tunnel.

## Build

On Omarchy/Arch, install the build and runtime dependencies. Install `gtk4`
only when building the GTK viewer.

```sh
omarchy pkg add rust base-devel pkgconf libvncserver libxkbcommon mesa
omarchy pkg add gtk4 # GTK viewer only
```

Build the viewer you want (or build both):

```sh
cargo build --locked --release -p omarchy-vnc-egui
cargo build --locked --release -p omarchy-vnc-gtk
```

The executables are written to `target/release/`. Builds require
LibVNCClient 0.9.15 or newer, discovered through `pkg-config`.

## Make a source package

Create a source archive from the current committed revision. Replace `0.2.0`
with the release version you are preparing:

```sh
git archive --format=tar.gz --prefix=omarchy-vnc-0.2.0/ \
  -o omarchy-vnc-0.2.0.tar.gz HEAD
```

The archive contains source only; build it on the target system using the
commands above. It intentionally does not include compiled binaries, saved
profiles, passwords, or native libraries.

## Install

After building, install one viewer or both into `~/.local`:

```sh
python3 scripts/install.py install egui
python3 scripts/install.py install gtk
python3 scripts/install.py install both
```

The installer adds a binary and application launcher for each selected viewer.
It changes only files it owns under the chosen prefix and never saves your
password. To install elsewhere, add `--prefix /path/to/prefix`. To remove a
viewer later (saved profiles are kept), run for example:

```sh
python3 scripts/install.py uninstall gtk
```

Ensure `~/.local/bin` is on your `PATH` before launching from a terminal.

## Connect

Start a viewer and add a connection in the terminal manager:

```sh
omarchy-vnc-egui
# or
omarchy-vnc-gtk
```

You may prefill an unsaved connection with `--host macmini.local` (and
optionally `--port 5900`). Use your Mac account's short username and password;
the Mac account must be permitted in Screen Sharing. An empty username selects
standard VNC authentication for compatible non-Mac servers.

Press `F5` to connect, `F6` to save a profile, and `F9` to reload profiles
changed by another instance. `Ctrl+Alt+Escape` releases remote input and leaves
fullscreen. Profiles are stored at
`$XDG_CONFIG_HOME/omarchy-vnc/profiles.json` (usually
`~/.config/omarchy-vnc/profiles.json`) and never contain passwords.

## More information

Run `omarchy-vnc-egui --help` or `omarchy-vnc-gtk --help` for command-line
options. Technical details, validation status, and license notices are in
[the architecture notes](docs/architecture.md),
[validation results](docs/validation.md), and
[THIRD_PARTY.md](THIRD_PARTY.md). Omarchy VNC is licensed under
[GPL-2.0-or-later](LICENSE).
