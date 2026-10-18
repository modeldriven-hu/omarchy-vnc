# Architecture

A Cargo workspace contains one toolkit-independent core, a shared Ratatui manager, and two independent binaries.
The core owns versioned non-secret profiles, conflict-safe persistence, VNC session state,
input messages, validated framebuffer snapshots, and an opaque LibVNCClient adapter.
Neither frontend depends on the other, and the core does not depend on either toolkit.

Each session owns a worker thread, a bounded input queue (256 messages), a unique ID,
and a latest-snapshot slot. If the input queue fills, the session disconnects rather
than dropping a key release. Frames replace the previous published snapshot at most
about 30 times per second; protocol updates are applied in LibVNCClient's canonical
framebuffer before publication, so coalescing never loses rectangle deltas.

The native shim bounds the framebuffer to 8192 pixels per dimension and 8,294,400 pixels
total (4K UHD pixel count). The Rust adapter independently checks dimensions before
constructing a pixel slice. LibVNCClient owns protocol decoding and ARD cryptography.
Its pointer and struct layout stay behind a small C shim compiled against the installed
headers; Rust sees an opaque pointer. The wrapper is not Send or Sync and stays on the
owning thread. Allocation, credentials, framebuffer copying, and teardown have explicit
ownership, documented at each unsafe boundary.

TCP establishment and DNS run on a private Tokio runtime with a 10-second connection
limit. A cancellation flag is checked during connection setup; after TCP establishment,
shutdown of a cloned socket interrupts blocked native reads, including authentication.
Native reads also have a two-second half-open read timeout. Closing a session joins its
worker. Resolver work may finish in the background inside the OS/runtime; it holds no
session credentials or GUI objects. Cancellation does not attempt to transfer sessions.

GTK uses a nonblocking Unix notification pipe and a GLib fd source. egui's worker wakeup
requests a repaint. Neither frontend polls for idle frames. The connected worker waits
up to 10 ms for protocol/input work. Toolkit textures and native events stay in frontends.

Profiles are shared in $XDG_CONFIG_HOME/omarchy-vnc/profiles.json (default ~/.config).
A lock covers the complete load/check/write operation; a revision check rejects stale
edits. Saves use a restricted temporary file, sync, and atomic rename. A corrupt or newer
format produces an error and is preserved. Reload is explicit. No password enters JSON.
Usernames are stored because they are connection settings, not authentication secrets.

Passwords are transient, redacted from diagnostics, and Rust-owned credential buffers
are zeroized. Native libraries/toolkit text widgets may make internal copies that this
application cannot guarantee to erase. Session callbacks never access GUI objects, and
asynchronous clipboard callbacks check the originating session ID before sending.

LibVNCClient negotiates remote cursor shapes. The shim validates cursor dimensions and
hotspots, converts BGRA plus the protocol mask into RGBA, and publishes owned cursor data.
egui draws a scaled software cursor over the remote viewport; GTK uses a native texture cursor.
A visible local pointer remains the fallback until a cursor shape arrives. No Apple-specific
adaptive video/audio, file transfer, or live session transfer between frontends is implemented.


## Terminal-to-viewer lifecycle (0.2)

Both binaries use omarchy-vnc-tui before initializing a graphical toolkit. The terminal
manager edits the existing core Model, keeps the password in a zeroizing transient buffer,
and starts Session::connect. Failures and cancellation stay in the terminal. A connected
session with a framebuffer is returned only after leaving raw mode, leaving/clearing the
alternate screen, and restoring the cursor. Normal terminal output/input modes are active
while the viewer runs.

Session::set_waker replaces a callback under a short mutex and immediately notifies the new
UI of already-published state. The worker clones the callback before invoking it outside the
lock. The session ID, socket, buffers, and protocol worker remain intact; no credentials pass
through process arguments, files, or IPC. Core integration tests retain the same session ID
and frame Arc when the wakeup target changes.

The selected GUI consumes that Session directly. In viewer-only mode, disconnect/close ends
the graphical event loop, and remote failure returns an error to the terminal Model. egui
uses eframe's returning/reusable event loop; GTK creates a new non-unique Application for each
viewing cycle. The outer main loop then enters a fresh terminal screen with the same profile
model. Closing a viewer disconnects its session rather than handing it to another binary.

--gui bypasses the terminal manager and uses the original graphical profile/authentication
screens. Desktop entries request a terminal. A direct non-terminal launch uses Omarchy's
styled terminal launcher. The terminal UI respects NO_COLOR and otherwise uses the shared
palette. Its centered layout fits within 110×28 cells, with a 60×24 minimum-size message.
