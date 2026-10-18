#!/usr/bin/env python3
"""Install or uninstall only files owned by Omarchy VNC; no root required."""
import argparse
import hashlib
import json
from pathlib import Path
import os
import tempfile

ROOT = Path(__file__).resolve().parent.parent

def digest(data):
    return hashlib.sha256(data).hexdigest()

def atomic(path, data, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as file:
            file.write(data)
            file.flush()
            os.fsync(file.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)

def operate(action, frontend, prefix, build_dir):
    prefix = prefix.expanduser().absolute()
    if any(c in str(prefix) for c in '\n\r'):
        raise ValueError("Prefix must not contain line breaks")
    state = prefix / "share/omarchy-vnc"
    manifest = state / f"{frontend}.json"
    previous = json.loads(manifest.read_text()) if manifest.exists() else {}
    binary_name = f"omarchy-vnc-{frontend}"
    binary_path = prefix / "bin" / binary_name
    app_id = "local.omarchy-vnc.egui" if frontend == "egui" else "local.omarchy_vnc.gtk"
    desktop = prefix / "share/applications" / f"{app_id}.desktop"
    icon = prefix / "share/icons/hicolor/scalable/apps/omarchy-vnc.svg"
    # Manifest paths are compared with the exact allowed destinations before use.
    native = prefix / "lib/omarchy-vnc/libvncclient.so.1"
    license_path = prefix / "share/licenses/omarchy-vnc/LICENSE"
    notices = prefix / "share/licenses/omarchy-vnc/THIRD_PARTY.md"
    shared = {icon, native, license_path, notices}
    allowed = {str(path) for path in [binary_path, desktop, *shared]}
    if not set(previous).issubset(allowed):
        raise ValueError("Invalid installation manifest; refusing changes")
    if action == "install":
        binary = build_dir / binary_name
        # Desktop Exec quoting is not shell quoting; % is a desktop field code.
        executable = str(binary_path).replace('\\', '\\\\').replace('"', '\\"').replace('`', '\\`').replace('$', '\\$').replace('%', '%%')
        entry = f'[Desktop Entry]\nType=Application\nName=Omarchy VNC ({frontend})\nComment=Connect to a remote desktop\nExec="{executable}"\nIcon=omarchy-vnc\nTerminal=true\nCategories=Network;RemoteAccess;\nStartupWMClass={app_id}\n'
        files = {binary_path: binary.read_bytes(), desktop: entry.encode(), icon: (ROOT / "assets/omarchy-vnc.svg").read_bytes(), license_path:(ROOT / "LICENSE").read_bytes(), notices:(ROOT / "THIRD_PARTY.md").read_bytes()}
        private_runtime = build_dir.parent / "lib/omarchy-vnc/libvncclient.so.1"
        if private_runtime.is_file():
            files[native] = private_runtime.read_bytes()
        # Check every destination before modifying any file.
        for path, data in files.items():
            if path.is_symlink():
                raise ValueError(f"Refusing to replace symlink: {path}")
            if path.exists() and digest(path.read_bytes()) != previous.get(str(path)):
                if path not in shared or path.read_bytes() != data:
                    raise ValueError(f"Existing file is unowned or modified: {path}")
        for path, data in files.items():
            atomic(path, data, 0o755 if path == binary_path else 0o644)
        atomic(manifest, json.dumps({str(path): digest(data) for path, data in files.items()}, indent=2).encode())
    else:
        other = state / ("gtk.json" if frontend == "egui" else "egui.json")
        for path_string, expected in previous.items():
            path = Path(path_string)
            if path in shared and other.exists() and str(path) in json.loads(other.read_text()):
                continue
            if path.is_symlink() or (path.exists() and digest(path.read_bytes()) != expected):
                raise ValueError(f"Installed file was modified; preserving it: {path}")
        for path_string in previous:
            path = Path(path_string)
            if path not in shared or not other.exists() or str(path) not in json.loads(other.read_text()):
                path.unlink(missing_ok=True)
        manifest.unlink(missing_ok=True)
    print(f"{action}: {frontend} under {prefix}")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["install", "uninstall"])
    parser.add_argument("frontend", choices=["egui", "gtk", "both"])
    parser.add_argument("--prefix", type=Path, default=Path.home() / ".local")
    parser.add_argument("--build-dir", type=Path, default=ROOT / "target/release")
    args = parser.parse_args()
    for frontend in (["egui", "gtk"] if args.frontend == "both" else [args.frontend]):
        operate(args.action, frontend, args.prefix, args.build_dir)

if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        raise SystemExit(str(error))
