# Project instructions

Implementation was explicitly authorized by the user on 2026-10-18. The project uses
Rust with a toolkit-independent core, a shared Ratatui terminal manager, egui/eframe
(OpenGL), and GTK4 viewers. The user authorized terminal-first operation on 2026-10-18: connect
in the terminal, attach the same session to the graphical viewer, then return to the terminal.
The --gui option preserves the original graphical connection form.
The user requested preserving the Mac's existing setup and supporting Apple account login;
LibVNCClient was selected because vnc-rs does not support that authentication.

Read README.md, docs/architecture.md, docs/backend.md, and tickets/README.md before work.
Keep graphical and terminal toolkit types out of the core. Build each frontend independently; egui must not
require GTK. Shared profiles must remain compatible and safe across concurrent processes.
Switching graphical toolkits means starting the other binary. Within each binary, the terminal
manager hands the same live session to its graphical viewer.

Keep credentials and real profiles out of Git. Never claim an actual Mac login or interactive
Wayland check passed without performing it. Preserve any pending validation in docs/validation.md.
Do not modify packaged Omarchy files. Restrict unsafe code to the native adapter with documented
ownership/bounds invariants. Preserve GPL and third-party license obligations.

Normal builds require libvncserver >=0.9.15 via pkg-config. A private development package was
extracted at /tmp/omarchy-vnc-native; use OMARCHY_VNC_NATIVE_ROOT for builds and LD_LIBRARY_PATH
for Cargo test executables if the system package is unavailable. Dependency cache for this
session is /tmp/omarchy-vnc-cargo. Never commit generated native libraries or target artifacts.
