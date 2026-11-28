#!/usr/bin/env python3
"""Real traffic for the policy LAB: one server behind the tunnel, a bulk sender and an
interactive probe in front of it.  Stdlib only, no product code involved.

One listening server is the tunnel's fixed destination, so every TCP stream the tunnel
opens lands here.  A stream announces its mode in its first byte:

  b'B' -> bulk: read exactly --bulk-size bytes, hash them, stamp sink-side t0/t1
  b'E' -> echo: 64-byte request/response round trips (the interactive flow)

Both flows therefore share one tunnel session and compete for it, which is the point:
the bulk transfer is the load and the echo round trips are what a user feels.

Roles
  server --port P --bulk-size N --json OUT     (the sink; runs on the listener host)
  send   --target H:P --size N --json OUT      (bulk sender; runs on the joiner host)
  probe  --target H:P --count N --json OUT     (interactive probe; runs with send)

Every number is measured from real sockets: the sink reports the bytes and the wall time
it actually held the bulk stream, the sender reports what it pushed, and the probe reports
per-round-trip microseconds.  Nothing here is derived from a constant.
"""
import argparse, hashlib, json, os, socket, sys, threading, time

CH = 1 << 16
ECHO = 64


def gen(n, seed=b"baft-policy-lab"):
    """Deterministic, incompressible-ish stream so the sink can verify what it received."""
    i, sent = 0, 0
    while sent < n:
        blk = hashlib.sha256(seed + i.to_bytes(8, "big")).digest() * (CH // 32)
        blk = blk[: min(CH, n - sent)]
        sent += len(blk)
        i += 1
        yield blk


def expected(n):
    h = hashlib.sha256()
    for b in gen(n):
        h.update(b)
    return h.hexdigest()


def pct(values, q):
    if not values:
        return None
    ordered = sorted(values)
    idx = min(len(ordered) - 1, max(0, int(round((len(ordered) - 1) * q))))
    return ordered[idx]


def write_json(path, obj):
    tmp = f"{path}.tmp.{os.getpid()}"
    with open(tmp, "w") as fh:
        json.dump(obj, fh)
    os.replace(tmp, path)


def server(port, bulk_size, out, idle):
    state = {"bulk_total": 0, "bulk_sha": None, "t0": None, "t1": None,
             "echo_streams": 0, "echo_trips": 0, "errors": []}
    lock = threading.Lock()
    done = threading.Event()

    def handle(conn):
        mode = b""
        try:
            conn.settimeout(idle)
            mode = conn.recv(1)
            if mode == b"B":
                h, n, t0 = hashlib.sha256(), 0, time.time()
                while n < bulk_size:
                    chunk = conn.recv(min(CH, bulk_size - n))
                    if not chunk:
                        break
                    h.update(chunk)
                    n += len(chunk)
                t1 = time.time()
                with lock:
                    state["bulk_total"] += n
                    state["bulk_sha"] = h.hexdigest()
                    state["t0"] = state["t0"] or t0
                    state["t1"] = t1
                conn.sendall(b"K" if n == bulk_size else b"X")
                if state["bulk_total"] >= bulk_size:
                    written = dict(state)
                    written["role"] = "server"
                    written["bulk_size"] = bulk_size
                    written["bulk_complete"] = True
                    write_json(out, written)
                    done.set()
            elif mode == b"E":
                with lock:
                    state["echo_streams"] += 1
                while True:
                    buf = b""
                    while len(buf) < ECHO:
                        part = conn.recv(ECHO - len(buf))
                        if not part:
                            return
                        buf += part
                    conn.sendall(buf)
                    with lock:
                        state["echo_trips"] += 1
            else:
                with lock:
                    state["errors"].append("first-byte:" + mode.hex())
        except Exception as exc:  # noqa: BLE001 - reported, never raised
            with lock:
                state["errors"].append(type(exc).__name__)
        finally:
            try:
                conn.close()
            except OSError:
                pass

    ls = socket.socket()
    ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    ls.bind(("0.0.0.0", port))
    ls.listen(16)
    ls.settimeout(1.0)
    print(json.dumps({"role": "server", "event": "listening", "port": port}), flush=True)
    # The bulk flow finishing does not end the run: the interactive flow is still using the
    # tunnel. Keep accepting echo streams until the bulk is complete AND the window has been
    # idle for --idle seconds, so a fast path cannot close the probe's connection early.
    gave_up = time.time() + idle
    while True:
        try:
            conn, _ = ls.accept()
        except socket.timeout:
            if done.is_set() and time.time() - state["t1"] > 2 * idle:
                break
            if not done.is_set() and time.time() > gave_up:
                break
            continue
        gave_up = time.time() + idle
        threading.Thread(target=handle, args=(conn,), daemon=True).start()
    time.sleep(0.3)
    final = dict(state)
    final["role"] = "server"
    final["bulk_size"] = bulk_size
    final["bulk_complete"] = state["bulk_total"] >= bulk_size
    write_json(out, final)
    print(json.dumps({"role": "server", "event": "finished", "bulk_total": state["bulk_total"],
                      "echo_trips": state["echo_trips"], "errors": len(state["errors"])}), flush=True)


def connect(target, timeout):
    host, port = target.rsplit(":", 1)
    sock = socket.create_connection((host, int(port)), timeout=timeout)
    sock.settimeout(timeout)
    return sock


def send(target, size, out, timeout):
    sock = connect(target, timeout)
    try:
        sock.sendall(b"B")
        t0 = time.time()
        for chunk in gen(size):
            sock.sendall(chunk)
        ack = sock.recv(1)
        elapsed = time.time() - t0
    finally:
        sock.close()
    result = {"role": "send", "size_bytes": size, "elapsed_s": round(elapsed, 3),
              "sender_MiB_per_s": round(size / elapsed / 1048576, 3) if elapsed else None,
              "ack": ack.decode(errors="replace"), "sha_expected": expected(size)}
    write_json(out, result)
    print(json.dumps(result), flush=True)


def probe(target, count, interval_ms, out, timeout):
    rtts, errors = [], 0
    sock = connect(target, timeout)
    try:
        sock.sendall(b"E")
        payload = bytes(range(ECHO))
        for _ in range(count):
            t0 = time.perf_counter()
            sock.sendall(payload)
            got = b""
            while len(got) < ECHO:
                part = sock.recv(ECHO - len(got))
                if not part:
                    raise ConnectionError("eof")
                got += part
            rtts.append((time.perf_counter() - t0) * 1e6)
            time.sleep(interval_ms / 1000.0)
    except Exception:  # noqa: BLE001 - counted, and the sample list is what it is
        errors += 1
    finally:
        sock.close()
    result = {"role": "probe", "count": count, "ok": len(rtts), "errors": errors,
              "p50_us": round(pct(rtts, 0.50), 1) if rtts else None,
              "p95_us": round(pct(rtts, 0.95), 1) if rtts else None,
              "p99_us": round(pct(rtts, 0.99), 1) if rtts else None,
              "max_us": round(max(rtts), 1) if rtts else None}
    write_json(out, result)
    print(json.dumps(result), flush=True)


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("role", choices=["server", "send", "probe"])
    ap.add_argument("--port", type=int)
    ap.add_argument("--bulk-size", type=int)
    ap.add_argument("--target")
    ap.add_argument("--size", type=int)
    ap.add_argument("--count", type=int, default=60)
    ap.add_argument("--interval-ms", type=float, default=50)
    ap.add_argument("--idle", type=float, default=45)
    ap.add_argument("--timeout", type=float, default=60)
    ap.add_argument("--json", required=True)
    a = ap.parse_args(argv)
    if a.role == "server":
        if not a.port or not a.bulk_size:
            ap.error("server needs --port and --bulk-size")
        server(a.port, a.bulk_size, a.json, a.idle)
    elif a.role == "send":
        if not a.target or not a.size:
            ap.error("send needs --target and --size")
        send(a.target, a.size, a.json, a.timeout)
    else:
        if not a.target:
            ap.error("probe needs --target")
        probe(a.target, a.count, a.interval_ms, a.json, a.timeout)


if __name__ == "__main__":
    main(sys.argv[1:])
