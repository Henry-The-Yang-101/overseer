#!/usr/bin/env python3
"""Exercise the real event loop, resizing, navigation, and terminal restoration."""
import fcntl, os, pathlib, select, struct, subprocess, termios, time
root = pathlib.Path(__file__).resolve().parents[1]
master, slave = os.openpty()
original = termios.tcgetattr(slave)
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 46, 160, 1440, 828))
env = dict(os.environ, TERM='xterm-256color', COLORTERM='truecolor')
proc = subprocess.Popen([str(root/'target/release/overseer'), '--demo', '--no-boot'], stdin=slave, stdout=slave, stderr=slave, env=env, close_fds=True)
output = bytearray()
def drain(seconds=0.15):
    deadline = time.monotonic()+seconds
    while time.monotonic() < deadline:
        if select.select([master], [], [], max(0, deadline-time.monotonic()))[0]:
            try: output.extend(os.read(master, 65536))
            except OSError: break
try:
    drain(.5)
    for keys in [b'1', b':inspect left speaker', b'\r', b'e', b'e', b'\x1b', b'p', b'\x1b', b'o', b'o', b'\t', b'z', b'\x1b', b'm', b'g', b'i', b'\x1b[C', b'\x1b[C', b'\r', b'\x1b[6~', b'\x1b[5~', b'\x1b[B', b'\x1b', b'\x1b[C', b'\x1b[C', b'\x1b[C', b'\x1b[C', b'\x1b[C', b'\x1b[C', b'\x1b[B', b'\r', b'\x1b[C', b'\x1b[B', b'\x1b', b'/rustc', b'\r', b'\x1b[B', b'f', b' ', b',', b' ', b'\x1b[F', b'?', b'\x1b', b':theme amber', b'\r', b'a', b'a']:
        os.write(master,keys);drain()
    for rows, cols in [(24,80),(14,52),(36,120),(55,200)]:
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',rows,cols,cols*9,rows*18));os.kill(proc.pid,28);drain(.2)
    for _ in range(7):os.write(master,b'\x1b');drain(.05)
    os.write(master,b'q');drain(.4);proc.wait(timeout=5)
    assert proc.returncode==0, bytes(output[-3000:]).decode(errors='replace')
    restored=termios.tcgetattr(slave)
    assert restored[3] & (termios.ICANON|termios.ECHO) == original[3] & (termios.ICANON|termios.ECHO), 'terminal modes not restored'
    assert b'\x1b[?1049l' in output, 'alternate screen not restored'
    assert b'panic' not in output.lower(), 'panic in terminal output'
    (root/'artifacts').mkdir(exist_ok=True)
    (root/'artifacts/smoke.ansi').write_bytes(output)
    print(f'PTY smoke passed: navigation, search, follow, replay, palette, motion, four sizes; terminal restored ({len(output)} bytes).')
finally:
    if proc.poll() is None:proc.terminate();proc.wait(timeout=5)
    os.close(master);os.close(slave)
