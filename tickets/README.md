# Development tickets

The user authorized implementation on 2026-10-18. Rust core, egui/OpenGL frontend,
GTK4 frontend, and LibVNCClient account authentication are implemented. The user
confirmed successful Apple login and Mac desktop rendering in egui.

See [validation results](../docs/validation.md) for evidence and the outstanding
interactive checks. “Implemented” does not mean unperformed acceptance checks passed.

| Ticket | Work | State |
| --- | --- | --- |
| [001](001.md) | Scope and session behavior | Done |
| [002](002.md) | Mac compatibility and backend selection | Apple login confirmed; extended checks pending |
| [003](003.md) | Workspace and UI-independent core | Done |
| [004](004.md) | Shared profiles | Done |
| [005](005.md) | Authentication and connection lifecycle | Implemented; manual checks pending |
| [006](006.md) | Framebuffer and geometry | Implemented; manual checks pending |
| [007](007.md) | Input, cursor, clipboard | Implemented; manual checks pending |
| [008](008.md) | Styling and launcher integration | Installed; theme review pending |
| [009](009.md) | Packaging and user installation | Done |
| [010](010.md) | End-to-end validation and documentation | Automated checks pass; manual matrix pending |
| [011](011.md) | egui/OpenGL frontend | Mac login/rendering confirmed; further checks pending |
| [012](012.md) | GTK4 frontend | Build and fixture rendering pass; Mac input checks pending |
| [013](013.md) | Form controls, password visibility, cursor feedback | Implemented; awaiting user confirmation |
| [014](014.md) | Terminal manager and connected graphical viewer | Implemented; local lifecycle checks pass |

The original dependency order is retained inside each ticket. Technical direction was
updated from vnc-rs to LibVNCClient after the real server probe and the user's request
to preserve Apple account authentication. Both frontends share profiles; switching UI
means starting the other binary, without transferring active sessions or credentials.
