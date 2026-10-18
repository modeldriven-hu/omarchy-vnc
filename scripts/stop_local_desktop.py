#!/usr/bin/env python3
"""Stop only the private Xvnc/xterm processes started for this development session."""
import json
import os
from pathlib import Path
import signal

root = Path('/tmp/omarchy-vnc-test-desktop')
record = root / 'session.json'
if not record.exists():
    raise SystemExit('No private test desktop session record found.')
state = json.loads(record.read_text())
for key, executable in [('terminal_pid', root / 'usr/bin/xterm'), ('server_pid', root / 'usr/bin/Xvnc')]:
    pid = state[key]
    try:
        command = Path(f'/proc/{pid}/cmdline').read_bytes().split(b'\0')[0]
        if command != os.fsencode(executable):
            print(f'Preserving process {pid}: its executable no longer matches the test session.')
            continue
        os.kill(pid, signal.SIGTERM)
        print(f'Stopped {executable.name} ({pid}).')
    except FileNotFoundError:
        pass
record.unlink(missing_ok=True)
