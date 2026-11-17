#!/usr/bin/env python3
"""Non-tunnel bulk-transfer control for LAB path diagnosis (no BAFT-TUNNEL code involved).

server: python3 tls-bulk-control.py server PORT MODE COUNT CERTDIR
client: python3 tls-bulk-control.py client HOST PORT MODE DIR SIZE ITER SNI [AGE_SECONDS]
DIR echo = send SIZE and read it back concurrently (like the integrity probe); AGE pings the flow first
MODE: tls | tcp. DIR: up (client sends) | down (server sends). SIZE in bytes.
One JSON line per transfer. Payload is random (incompressible). No secrets involved.
The client skips certificate verification on purpose: the server is a throwaway self-signed
diagnostic endpoint and only the transport behaviour is measured.
"""
import json, os, socket, ssl, subprocess, sys, time

CHUNK = 16384
STALL = 15  # seconds without progress counts as a stall


def log(**kw):
    print(json.dumps(kw), flush=True)


def server(port, mode, count, certdir):
    ctx = None
    if mode == "tls":
        crt, key = os.path.join(certdir, "c.pem"), os.path.join(certdir, "k.pem")
        subprocess.run(["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1",
                        "-nodes", "-keyout", key, "-out", crt, "-subj", "/CN=lab-control", "-days", "1"],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(crt, key)
    ls = socket.socket()
    ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    ls.bind(("0.0.0.0", port))
    ls.listen(8)
    ls.settimeout(120)
    log(ev="listening", port=port, mode=mode)
    for _ in range(count):
        t0 = time.time()
        try:
            c, _a = ls.accept()
        except socket.timeout:
            log(ev="accept-timeout"); return
        n, err = 0, None
        try:
            c.settimeout(STALL)
            if ctx:
                c = ctx.wrap_socket(c, server_side=True)
            while True:
                hdr = b""
                while not hdr.endswith(b"\n"):
                    b1 = c.recv(1)
                    if not b1:
                        raise ConnectionError("eof-before-header")
                    hdr += b1
                if hdr == b"p\n":  # application keepalive while the flow ages
                    c.sendall(b"q\n")
                    continue
                break
            d, size = hdr.decode().split(); size = int(size)
            if d == "echo":
                while n < size:
                    b = c.recv(CHUNK)
                    if not b: break
                    c.sendall(b); n += len(b)
            elif d == "up":
                while n < size:
                    b = c.recv(CHUNK)
                    if not b: break
                    n += len(b)
                c.sendall(b"K")
            else:
                buf = os.urandom(CHUNK)
                while n < size:
                    k = min(CHUNK, size - n); c.sendall(buf[:k]); n += k
            c.close()
        except Exception as e:
            err = type(e).__name__ + ":" + str(getattr(e, "errno", ""))
        log(ev="srv", bytes=n, secs=round(time.time() - t0, 2), err=err)


def client(host, port, mode, d, size, iters, sni, age=0):
    ok = 0
    for i in range(iters):
        t0 = time.time(); n = 0; err = None
        try:
            s = socket.create_connection((host, port), timeout=STALL)
            s.settimeout(STALL)
            if mode == "tls":
                ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
                ctx.check_hostname = False; ctx.verify_mode = ssl.CERT_NONE
                s = ctx.wrap_socket(s, server_hostname=sni or None)
            t_age = time.time()
            while time.time() - t_age < age:  # age the flow with 5 s app pings (like yamux keepalive)
                s.sendall(b"p\n")
                if s.recv(2) != b"q\n":
                    raise RuntimeError("bad-pong")
                time.sleep(5)
            t0 = time.time()
            s.sendall(f"{d} {size}\n".encode())
            if d == "echo":
                import threading
                def tx():
                    sent = 0; buf = os.urandom(CHUNK)
                    while sent < size:
                        k = min(CHUNK, size - sent); s.sendall(buf[:k]); sent += k
                th = threading.Thread(target=tx, daemon=True); th.start()
                while n < size:
                    b = s.recv(CHUNK)
                    if not b: break
                    n += len(b)
                th.join(STALL)
            elif d == "up":
                buf = os.urandom(CHUNK)
                while n < size:
                    k = min(CHUNK, size - n); s.sendall(buf[:k]); n += k
                if s.recv(1) != b"K":
                    raise RuntimeError("no-ack")
            else:
                while n < size:
                    b = s.recv(CHUNK)
                    if not b: break
                    n += len(b)
            s.close()
        except Exception as e:
            err = type(e).__name__ + ":" + str(getattr(e, "errno", ""))
        done = err is None and n >= size
        ok += done
        log(ev="cli", mode=mode, dir=d, age=age, i=i, size=size, bytes=n, secs=round(time.time() - t0, 2), err=err, ok=done)
        time.sleep(1)
    log(ev="summary", mode=mode, dir=d, ok=ok, total=iters)


if __name__ == "__main__":
    a = sys.argv[1:]
    if a[0] == "server":
        server(int(a[1]), a[2], int(a[3]), a[4])
    else:
        client(a[1], int(a[2]), a[3], a[4], int(a[5]), int(a[6]), a[7] if len(a) > 7 else "", int(a[8]) if len(a) > 8 else 0)
