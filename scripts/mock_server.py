#!/usr/bin/env python3
"""Loopback-only RFB fixture with a generated framebuffer; no real desktop or credentials."""
import argparse
import socket
import struct
import threading
import time


def exact(connection, count):
    data = bytearray()
    while len(data) < count:
        chunk = connection.recv(count - len(data))
        if not chunk:
            raise EOFError
        data.extend(chunk)
    return bytes(data)


def serve(connection):
    width, height = 640, 400
    with connection:
        connection.settimeout(15)
        connection.sendall(b'RFB 003.008\n')
        exact(connection, 12)
        connection.sendall(bytes([1, 1]))
        if exact(connection, 1) != b'\x01':
            return
        connection.sendall(bytes(4))
        exact(connection, 1)
        pixel_format = bytes([32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0])
        title = b'Omarchy VNC local fixture'
        connection.sendall(struct.pack('!HH', width, height) + pixel_format + struct.pack('!I', len(title)) + title)
        pixels = bytearray()
        for y in range(height):
            for x in range(width):
                pixels.extend((70 + (x // 32 % 2) * 30, 40 + y * 120 // height, 30 + x * 160 // width, 0))
        first = True
        while True:
            kind = exact(connection, 1)[0]
            if kind == 0:
                exact(connection, 19)
            elif kind == 2:
                header = exact(connection, 3)
                exact(connection, int.from_bytes(header[1:], 'big') * 4)
            elif kind == 3:
                exact(connection, 9)
                if first:
                    cursor_pixels=bytes([255,255,255,0])*256
                    cursor_mask=bytearray()
                    for y in range(16):
                        bits=sum(1 << (15-x) for x in range(16) if (y<12 and x<=y//2) or (10<=y and 3<=x<=5))
                        cursor_mask.extend(bits.to_bytes(2,'big'))
                    cursor=struct.pack('!HHHHi',0,0,16,16,-239)+cursor_pixels+cursor_mask
                    connection.sendall(b'\0\0\0\2' + struct.pack('!HHHHi', 0, 0, width, height, 0) + pixels + cursor)
                    first = False
                else:
                    time.sleep(0.04)
                    connection.sendall(bytes(4))
            elif kind == 4:
                exact(connection, 7)
                print('key event received', flush=True)
            elif kind == 5:
                exact(connection, 5)
            elif kind == 6:
                header = exact(connection, 7)
                length = int.from_bytes(header[3:], 'big')
                if length > 65536:
                    return
                exact(connection, length)
                print('clipboard event received', flush=True)
            else:
                return


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, default=5909)
    args = parser.parse_args()
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(('127.0.0.1', args.port))
    listener.listen()
    print(f'Fixture listening on 127.0.0.1:{args.port}', flush=True)
    while True:
        connection, _ = listener.accept()
        def run(client=connection):
            try:
                serve(client)
            except (OSError, EOFError):
                pass
        threading.Thread(target=run, daemon=True).start()

if __name__ == '__main__':
    main()
