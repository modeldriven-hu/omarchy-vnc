# Backend decision

Decision: use **LibVNCClient 0.9.15 or later** through a narrow C adapter and Rust ownership wrapper.

| Candidate | Benefits | Finding |
| --- | --- | --- |
| vnc-rs 0.6 | Pure Rust, async interface, bounded protocol queues and decoder limits, MIT/Apache licensing | Source inspection found standard VNC password/no-auth only; unsuitable for the target's current authentication |
| LibVNCClient | Existing ARD type 30 account authentication, broad encoding support | Selected; adds a native library and GPL-2.0-or-later license obligations |

A credential-free probe of the actual supplied Mac hostname on 2026-10-18 returned
RFB 003.889 and security offers 30, 33, 36, 35. No standard password type 2 was offered.
The user requested keeping the Mac setup and supporting Apple account login.
The application therefore selects only type 30 when a username is entered; it does
not silently downgrade that connection to password-only or unauthenticated access.
With an empty username, it supports standard VNC password and no-auth servers for
interoperability and local fixtures. No TLS security types are enabled by this adapter.

ARD protects the credential exchange using the upstream library's implementation.
That does **not** mean framebuffer/input traffic is encrypted. The application labels
sessions as unencrypted. Use a trusted local network or a separately secured transport.

Local tests exercise a known-answer standard VNC password response, framebuffer updates,
remote input, clipboard messages, rejected authentication, and stalled-handshake cancellation.
An ARD fixture offers the same authentication types as the Mac, confirms type 30 is
selected, supplies a Diffie–Hellman challenge, receives the encrypted response, and
rejects intentionally. This checks protocol negotiation; it does not prove that the
user's real Mac credentials or complete desktop session work.

The user confirmed successful Apple account login and Mac desktop rendering in egui.
The actual macOS version, complete input mapping on the user's layout, updated cursor behavior,
locked/headless behavior, and long-session performance remain to be verified interactively.
Do not claim these checks have passed until recorded in validation.md.
