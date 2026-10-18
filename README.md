# Omarchy VNC

## Screenshot

<img width="1598" height="860" alt="image" src="https://github.com/user-attachments/assets/5a148296-28e4-48f1-81c2-a552423843a3" />

## Description

A Rust VNC application for Omarchy, with a shared core, a **Ratatui terminal connection manager**, and two graphical viewers:

- **omarchy-vnc-egui** — egui/eframe with OpenGL; no GTK dependency.
- **omarchy-vnc-gtk** — GTK4 through gtk-rs.

Both use **LibVNCClient** for embedded VNC and Apple Remote Desktop account authentication.
Both share the same terminal interface and saved profiles; switch viewers by launching the other binary.
Active sessions are independent and are not transferred between interfaces.

## Terminal-first workflow

```sh
./target/release/omarchy-vnc-egui --host macmini.local
# Or:
./target/release/omarchy-vnc-gtk --host macmini.local
```

The local release builds include a private development copy of LibVNCClient under
`target/lib/omarchy-vnc`, so they run without installing that library system-wide.
Other native graphics/library dependencies still come from the system.

The default interface opens in a terminal using Omarchy's palette and terminal font.
Choose a profile with **↑/↓**, press **Tab** to edit its fields, and **F5** to connect.
Enter your Mac account's **short username** and password for Apple login; your account
must be allowed in the Mac's Screen Sharing settings. **F2** shows/hides the password.
Saved Apple profiles prompt for the transient password before starting a connection.

The terminal displays connection progress, errors, and cancellation (**Esc**). A graphical
window opens only after the session is established and a framebuffer is available.
The same live session moves into the viewer: there is no second login, password IPC, or
socket reconnection. Closing the viewer or disconnecting returns to the terminal manager;
you can connect again in the same process. The terminal stays open in the background while
the viewer runs, with normal terminal input modes restored.

The original graphical connection form remains available:

```sh
omarchy-vnc-egui --gui --host macmini.local
omarchy-vnc-gtk --gui --host macmini.local
```

No password is saved, accepted as a command-line argument, or needed in this repository.

An empty username selects standard VNC password authentication for other servers.
Apple account authentication protects the login exchange; this implementation does not
encrypt desktop/input traffic. Use a trusted LAN or a separately configured secure transport.

## Build

On Omarchy/Arch, install the dependencies for the desired frontend:

```sh
omarchy pkg add rust base-devel pkgconf libvncserver libxkbcommon mesa
# GTK frontend additionally needs:
omarchy pkg add gtk4
```

If using rustup, select a recent stable toolchain (workspace minimum: Rust 1.92; tested with 1.99.1).
Cargo resolves and locks actual dependency versions in Cargo.lock.

```sh
cargo build --locked --release -p omarchy-vnc-egui
cargo build --locked --release -p omarchy-vnc-gtk
```

The egui package does not build or link GTK. Normal builds discover LibVNCClient >=0.9.15
through pkg-config. To develop without a system installation, extract a trusted compatible
Arch `libvncserver` package into a private directory and set
`OMARCHY_VNC_NATIVE_ROOT=/absolute/extracted/root` during the build. The build copies only
`usr/lib/libvncclient.so.1` into `target/lib/omarchy-vnc`; frontend binaries locate it using
a path relative to themselves. Do not commit extracted packages or binaries.

## Usage

Add/edit a connection, choose hostname/IP and an explicit port, then save it with **F6** if desired.
A default profile is selected on startup; it never connects automatically. `--host HOST`
prepopulates a new unsaved connection; `--port PORT` overrides its port; `--profile NAME` selects an existing saved profile.
Both binaries support `--help` and `--version` without opening a window.

Profiles live in `$XDG_CONFIG_HOME/omarchy-vnc/profiles.json` (default
`~/.config/omarchy-vnc/profiles.json`). They include the username and display/input preferences,
never the password. Conflicting writes from another window are rejected; press **F9** (or click **Reload** in `--gui` mode)
to read the latest version before editing again. Deleting a profile affects both frontends.


| Terminal shortcut | Action |
| --- | --- |
| ↑ / ↓ | Select profiles or fields |
| Tab / Shift+Tab | Move between profiles and editable fields |
| n / e | New / edit profile, while the profile list is focused |
| F2 | Show/hide password |
| F5 / Enter in password | Connect |
| F6 / F7 | Save / make default |
| F8 / Delete in profile list | Remove profile, with confirmation |
| F9 | Reload shared profiles |
| Esc | Cancel connection or restore edits |
| Ctrl+Q / Ctrl+C | Quit the terminal manager |
| Space | Toggle a selected option |

Text fields support Unicode-aware left/right, Home/End, Backspace/Delete, Ctrl+U,
and bracketed paste. Small terminals show a resize message instead of breaking layout.
`NO_COLOR` is respected; otherwise the active Omarchy palette colors the borders,
selection, and shortcut bar.

Click inside the remote desktop to capture input. **Ctrl+Alt+Escape** releases tracked keys
and pointer buttons and leaves fullscreen. Super-based compositor shortcuts remain local.
The default **Map Ctrl to Mac Command** setting makes Ctrl+C behave as Command+C on the Mac;
disable it to send ordinary Control. Alt corresponds to Option. Compositor-reserved shortcuts
cannot be forwarded. Keyboard layout/IME behavior still needs validation on your actual Mac.

Use **Fit** / **Fit to window** to scale the desktop, or disable it for a scrollable view at
one source pixel per logical UI pixel. Fullscreen is local display behavior, not a request
to change the Mac's resolution. Remote cursor shapes are displayed by each frontend, with a visible local pointer as fallback.

Text clipboard sharing is **off by default**. When enabled, remote text is received
automatically; **Send local clipboard to Mac** explicitly sends local text. The initial
implementation supports Latin-1 text up to 64 KiB. It does not provide file transfer or
continuous local clipboard monitoring. In egui, framework clipboard shortcuts may depend
on local clipboard contents; use explicit send and the Mac's Edit menu if needed.

## Install or uninstall

These commands modify only owned files under the chosen prefix (default `~/.local`):

```sh
python3 scripts/install.py install both
# Either frontend can be installed independently:
python3 scripts/install.py install egui
python3 scripts/install.py uninstall gtk
```

The installer adds separate application launcher entries that open the terminal manager and preserves shared profiles.
A bundled development library, when present, is installed into `~/.local/lib/omarchy-vnc`;
it is retained while either frontend uses it. Uninstall refuses to remove changed files.
No packaged Omarchy files or existing Hyprland bindings are changed.

An optional keybinding can be added to your existing `~/.config/hypr/bindings.lua`:

```lua
o.bind("SUPER + CTRL + V", "Omarchy VNC", { launch = "omarchy-vnc-egui" })
```

Check for an existing binding first; choose a free shortcut or explicitly unbind the old
one. This example is documentation only and has not been installed. Ensure `~/.local/bin`
is on PATH. Validate any manual Hyprland config change using `hyprctl reload` and
`hyprctl configerrors`.

## Verification and limitations

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/test_terminal_flow.py  # optional: Hyprland + local server at 127.0.0.1:5909
python3 -m unittest discover -s scripts -p 'test_*.py'
```

Protocol integration tests use loopback TCP sockets. With a private development library,
set `LD_LIBRARY_PATH=/path/to/extracted/root/usr/lib` for Cargo test executables.
A generated desktop fixture is available with `python3 scripts/mock_server.py`; connect
an empty-username profile to `127.0.0.1:5909`. It exposes no actual desktop or credentials.

See [validation results](docs/validation.md) for completed checks and outstanding Mac testing,
[architecture](docs/architecture.md), [backend selection](docs/backend.md), and
[development tickets](tickets/README.md).

A separate **local TigerVNC desktop** is running for this development session at
`127.0.0.1:5909`, with a terminal inside it. Connect with username and password empty,
and disable “Map Ctrl to Mac Command” for this Linux session. It is bound to loopback.
Its private files and process record are under `/tmp/omarchy-vnc-test-desktop`; stop it
with `python3 scripts/stop_local_desktop.py`. This is a temporary test session, not a
server installed into your normal desktop. To check its protocol/cursor path without a UI:

```sh
cargo run -p omarchy-vnc-core --example local_probe -- 5909
```

License: **GPL-2.0-or-later**. See [LICENSE](LICENSE) and [third-party notices](THIRD_PARTY.md).
