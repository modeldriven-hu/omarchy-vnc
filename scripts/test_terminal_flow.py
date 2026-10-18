#!/usr/bin/env python3
"""Optional Hyprland smoke test: private PTY → existing local VNC → viewer → PTY, twice.

Requires the loopback VNC server (default port 5909). It types only into its own PTY,
closes only its own viewer windows, and never uses real profiles or credentials.
"""
import argparse
import errno
import fcntl
import json
import os
from pathlib import Path
import select
import socket
import struct
import subprocess
import tempfile
import termios
import time
import threading

ROOT = Path(__file__).resolve().parent.parent

def clients(pid):
    result = subprocess.check_output(['hyprctl', '-j', 'clients'])
    return [client for client in json.loads(result) if client['pid'] == pid]

def run(frontend, port):
    with tempfile.TemporaryDirectory(prefix='omarchy-vnc-tui-check-') as temporary:
        directory = Path(temporary)
        config = directory / 'omarchy-vnc'
        config.mkdir()
        # Bound but not listening: a deterministic, locally refused connection.
        refused = socket.socket()
        refused.bind(('127.0.0.1', 0))
        failure_port = refused.getsockname()[1]
        stalled = socket.socket()
        stalled.bind(('127.0.0.1', 0))
        stalled.listen()
        stalled.settimeout(15)
        stalled_port = stalled.getsockname()[1]
        cancelled = threading.Event()
        def stall():
            try:
                connection, _ = stalled.accept()
                with connection:
                    connection.settimeout(15)
                    if connection.recv(1) == b'':
                        cancelled.set()
            except OSError:
                pass
        threading.Thread(target=stall, daemon=True).start()
        profiles = [{'name': name, 'host': '127.0.0.1', 'port': value, 'username': '',
                     'map_ctrl_to_command': False, 'clipboard': False, 'fullscreen': False}
                    for name, value in [('Local test', port), ('Unavailable test', failure_port), ('Stalled test', stalled_port)]]
        (config / 'profiles.json').write_text(json.dumps({'version': 1, 'revision': 0,
                                                        'default': 'Local test', 'profiles': profiles}))
        master, slave = os.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 32, 120, 0, 0))
        original = termios.tcgetattr(slave)
        environment = os.environ.copy()
        environment.update(XDG_CONFIG_HOME=str(directory), TERM='xterm-256color')
        def controlling_terminal():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)
        process = subprocess.Popen([str(ROOT / f'target/release/omarchy-vnc-{frontend}')],
                                   stdin=slave, stdout=slave, stderr=slave, env=environment,
                                   preexec_fn=controlling_terminal)
        output = bytearray()
        def drain(timeout=0.05):
            if select.select([master], [], [], timeout)[0]:
                try:
                    output.extend(os.read(master, 131072))
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
        def until(predicate, message, timeout=15):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                drain()
                if predicate():
                    return
                if process.poll() is not None:
                    raise AssertionError(f'{frontend}: exited early ({process.returncode}): {output.decode(errors="replace")[-1500:]}')
            raise AssertionError(f'{frontend}: timed out: {message}; {output.decode(errors="replace")[-1500:]}')
        try:
            until(lambda: b'O M A R C H Y' in output, 'terminal manager startup')
            assert not clients(process.pid), 'A viewer appeared before connecting'
            # Select the unavailable profile and connect; errors must remain in the terminal.
            os.write(master, b'\x1b[B\x1b[15~')
            until(lambda: b'Cannot reach host' in output, 'connection failure in terminal')
            assert not clients(process.pid), 'Connection failure opened a viewer'
            os.write(master, b'\x1b[B\x1b[15~')
            until(lambda: b'Connecting to' in output, 'connection progress')
            assert not clients(process.pid), 'A stalled handshake opened a viewer'
            os.write(master, b'\x1b')
            until(lambda: b'Connection cancelled' in output and cancelled.is_set(), 'Esc cancellation')
            os.write(master, b'\x1b[B')
            for cycle in range(2):
                before = output.count(b'\x1b[?1049h')
                os.write(master, b'\x1b[15~')
                until(lambda: bool(clients(process.pid)), f'viewer startup {cycle + 1}')
                window = clients(process.pid)[0]
                # Verify the terminal returned to cooked mode before viewer use.
                current = termios.tcgetattr(slave)
                assert current[3] & (termios.ECHO | termios.ICANON) == original[3] & (termios.ECHO | termios.ICANON)
                command = 'hl.dsp.window.close({window=' + json.dumps('address:' + window['address']) + '})'
                subprocess.run(['hyprctl', 'dispatch', command], capture_output=True, check=True)
                until(lambda: not clients(process.pid) and output.count(b'\x1b[?1049h') > before,
                      f'return to terminal {cycle + 1}')
            os.write(master, b'\x11')
            until(lambda: process.poll() is not None, 'clean Ctrl+Q exit')
            assert process.returncode == 0
            final = termios.tcgetattr(slave)
            assert final[3] & (termios.ECHO | termios.ICANON) == original[3] & (termios.ECHO | termios.ICANON)
            assert b'--gui' not in output, 'Unexpected terminal setup failure'
            print(f'{frontend}: terminal-only failure/cancellation, connected viewer, two return/reconnect cycles, terminal restoration: PASS')
        finally:
            (Path('/tmp') / f'omarchy-vnc-{frontend}-terminal-smoke.ansi').write_bytes(output)
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            refused.close()
            stalled.close()
            os.close(master)
            os.close(slave)

def gui_mode(frontend):
    with tempfile.TemporaryDirectory(prefix='omarchy-vnc-gui-check-') as temporary:
        environment=os.environ.copy()
        environment['XDG_CONFIG_HOME']=temporary
        with tempfile.TemporaryFile() as log:
            process=subprocess.Popen([str(ROOT/f'target/release/omarchy-vnc-{frontend}'),'--gui','--host','127.0.0.1'],stdin=subprocess.DEVNULL,stdout=log,stderr=log,env=environment)
            try:
                deadline=time.monotonic()+15
                while time.monotonic()<deadline and not clients(process.pid):
                    if process.poll() is not None:
                        log.seek(0)
                        raise AssertionError(log.read().decode(errors='replace'))
                    time.sleep(.1)
                windows=clients(process.pid)
                assert windows,'--gui did not open the graphical connection form'
                command='hl.dsp.window.close({window='+json.dumps('address:'+windows[0]['address'])+'})'
                subprocess.run(['hyprctl','dispatch',command],capture_output=True,check=True)
                assert process.wait(timeout=5)==0
                print(f'{frontend}: --gui without a terminal: PASS')
            finally:
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--frontend', choices=['egui', 'gtk', 'both'], default='both')
    parser.add_argument('--port', type=int, default=5909)
    arguments = parser.parse_args()
    for name in (['egui', 'gtk'] if arguments.frontend == 'both' else [arguments.frontend]):
        run(name, arguments.port)
        gui_mode(name)
