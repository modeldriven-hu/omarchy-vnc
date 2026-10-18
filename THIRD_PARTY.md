# Third-party software

Omarchy VNC is licensed under GPL-2.0-or-later because it links LibVNCClient.
The LICENSE file contains the GNU GPL version 2 text; the project grants the option to use any later GPL version.

- LibVNCClient 0.9.15: GPL-2.0-or-later; https://github.com/LibVNC/libvncserver . Source releases: https://github.com/LibVNC/libvncserver/releases .
- Ratatui and Crossterm: MIT; https://github.com/ratatui/ratatui and https://github.com/crossterm-rs/crossterm .
- egui/eframe: MIT OR Apache-2.0; https://github.com/emilk/egui .
- gtk-rs: MIT; GTK4 native libraries: LGPL-2.1-or-later; https://gtk-rs.org/ .
- Other Rust dependencies and versions are recorded in Cargo.lock. Use `cargo metadata --format-version 1` for package license metadata.

Normal builds link the system LibVNCClient library. A development build with
OMARCHY_VNC_NATIVE_ROOT copies that root's libvncclient.so.1 to target/lib/omarchy-vnc;
the frontend executables search this private path relative to themselves. The installer
copies this library when present, keeping the system library directories untouched.
The local development copy was extracted from the Omarchy Arch package
libvncserver-0.9.15-1-x86_64.pkg.tar.zst. Its other native dependencies remain system dependencies.

Before redistributing binaries, include the required notices/licenses and provide the
corresponding GPL-covered source and build materials. This repository has not published binaries.
