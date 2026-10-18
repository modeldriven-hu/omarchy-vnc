# Validation record — 2026-10-18

## Environment

Omarchy / Hyprland desktop; Rust 1.99.1; GTK 4.22.4; gtk-rs 0.10.3;
eframe/egui 0.33.3 with Glow/OpenGL; LibVNCClient 0.9.15.
The native package installation was dismissed at the authentication prompt, so the
local builds and user installation use a private library copy extracted from the
Omarchy Arch package. No system library directories or Omarchy configuration were changed.

## Completed

- `cargo fmt --all --check`.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`.
- `cargo test --workspace --locked --offline`: core/profile tests and live loopback protocol fixtures.
- `cargo build --release --workspace --offline`: both release binaries.
- Python installer tests: independent frontends, shared assets, paths with spaces,
  and refusal to overwrite/remove unrelated or modified files.
- Installation of both release binaries to a temporary prefix containing spaces;
  both run with their private library; removing egui preserves the working GTK binary;
  removing the last frontend removes shared owned assets.
- Installation of both release builds and separate launcher entries under `~/.local`.
- Dependency tree and runtime linkage inspection: the egui frontend has no GTK/GDK/GLib
  dependency; the core has no UI toolkit dependencies. Native graphics/VNC libraries remain dependencies.
- Both frontends mapped windows in the installed Hyprland session and displayed the
  generated loopback framebuffer. egui's updated cursor build also completed the fixture smoke check.
- Real TigerVNC 1.16.2 separate local desktop: the core received a 1280×800 framebuffer
  and a 9×16 rich cursor with hotspot [4, 8]. The server remains running on loopback port
  5909 for user tests, with a private xterm session; no system server installation.
- Protocol tests cover a known-answer standard VNC password response, rejected login,
  ARD type 30 selection and encrypted challenge response, framebuffer pixel conversion,
  cursor mask/hotspot conversion, clipboard traffic, key traffic, and prompt cancellation
  during a stalled handshake. The ARD fixture intentionally rejects its synthetic login.
- Profile tests cover corruption, invalid addresses, stale revisions, shared profile round trips,
  default-profile rename/deletion consistency, and preservation of newer edits.
- A credential-free real Mac probe returned RFB 003.889 and security types 30, 33, 36, 35.
- **The user confirmed that Apple account login succeeded and the real Mac desktop appeared
  in the egui frontend.** Credentials were entered directly into the application, not tools or Git.

Commands used `CARGO_HOME=/tmp/omarchy-vnc-cargo` and
`OMARCHY_VNC_NATIVE_ROOT=/tmp/omarchy-vnc-native`. Test executables used
`LD_LIBRARY_PATH=/tmp/omarchy-vnc-native/usr/lib`; loopback tests required network
access outside the execution sandbox. Running those tests inside the restricted
sandbox fails at socket creation, not at the application protocol layer.

## Feedback addressed

The user reported small fields and requested password visibility. Both frontends now
have larger fields and an explicit Show/Hide button; passwords start masked.
The user also reported an invisible remote pointer. The application now requests
remote cursor shapes, decodes their pixel masks/hotspots, and renders them in each UI.
A visible local cursor remains until a valid shape arrives. The native cursor path is
covered by the loopback regression test. A later independent Mac connection probe returned “No route to host.” A local TigerVNC
session and client were opened so UI/input testing can continue independently of the Mac.
The updated egui build was opened for the user
without closing the prior session or transferring any password.

## Pending interactive checks

- User confirmation of the updated form/password controls and cursor on the actual Mac.
- Full keyboard layout/IME, modifier/Command behavior, pointer drag/scroll, fullscreen,
  and local escape behavior on the Mac, including focus loss and reconnect.
- Actual Mac login using the GTK frontend (its shared core authentication and local
  framebuffer rendering passed; this does not substitute for its interactive input checks).
- Clipboard permission behavior with real applications, including Latin-1 limitations.
- Dark/light theme visual review and keyboard-only navigation of every screen in both UIs.
- Long-session CPU/memory/GPU measurements, macOS version, locked/headless behavior,
  display changes, and multiple-monitor limitations.

Pending checks remain open in the development tickets. This is a working initial
implementation, not a claim that every planned acceptance criterion has passed.


## Terminal-first release (0.2)

The user authorized replacing the default connection form with a shared terminal manager,
while retaining --gui and both viewers. The following local checks passed:

- Shared Ratatui package builds without either graphical toolkit.
- Masked/revealed rendering, small terminal handling, Unicode/grapheme editing,
  cancelled edits, clearing a full text field, and password prompting for saved Apple profiles.
- Session handoff keeps the same session ID and published frame buffer; the callback
  target changes without another connection or authentication exchange.
- Private PTY smoke tests for **both** frontends: no viewer before connection, failed
  connection stays in the terminal, stalled-handshake cancellation opens no viewer,
  live local VNC session opens its viewer, window close
  returns to the terminal, and a second connect/close cycle succeeds in the same process.
- Both frontends also open and close successfully with --gui and no controlling terminal.
- Terminal ECHO/ICANON modes are restored while the viewer runs and after Ctrl+Q exits.
- Visual inspection of the terminal layout with synthetic profiles in Foot. No credentials
  or real profile data were used in automated terminal/UI checks.

Re-run the optional lifecycle checks with `python3 scripts/test_terminal_flow.py` while the
local server is running; it controls only its own private PTYs and viewer windows.
Real Mac login was confirmed in the earlier graphical workflow; the new terminal-first Mac
login/keyboard workflow has not yet been confirmed interactively by the user.
