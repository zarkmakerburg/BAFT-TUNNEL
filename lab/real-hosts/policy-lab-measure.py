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
             "echo_streams": 0, "echo_trips": 0, "errors": [],
             "active": 0, "last": time.time()}
    lock = threading.Lock()

    def handle(conn):
        mode = b""
        with lock:
            state["active"] += 1
            state["last"] = time.time()
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
                    with lock:
                        state["last"] = time.time()
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
                        state["last"] = time.time()
            else:
                with lock:
                    state["errors"].append("first-byte:" + mode.hex())
        except Exception as exc:  # noqa: BLE001 - reported, never raised
            with lock:
                state["errors"].append(type(exc).__name__)
        finally:
            with lock:
                state["active"] -= 1
                state["last"] = time.time()
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
    # The deadline is inactivity on the sockets, never "no new connection": a slow bulk
    # transfer holds one stream open for minutes, and an accept-side deadline would cut it in
    # half and report a truncated sink. The server ends when nothing has been received for
    # --idle seconds.
    while True:
        try:
            conn, _ = ls.accept()
        except socket.timeout:
            with lock:
                quiet = state["active"] == 0 and time.time() - state["last"] > idle
            if quiet:
                break
            continue
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


def send(target, size, out, timeout, start_file=None, wait_file=None, wait_seconds=30.0):
    sock = connect(target, timeout)
    try:
        sock.sendall(b"B")
        if start_file:
            # The bulk flow is connected and about to stream: this is the moment the probe
            # should start sampling, not a sleep in the orchestrator that a fast path outruns.
            open(start_file, "w").close()
        waited = 0.0
        if wait_file:
            # Hold the first byte until the probe's echo stream is connected, so every sample
            # it takes overlaps this transfer. Bounded: a broken probe must not hang the run.
            began = time.time()
            while not os.path.exists(wait_file) and time.time() - began < wait_seconds:
                time.sleep(0.01)
            waited = round(time.time() - began, 3)
        t0 = time.time()
        for chunk in gen(size):
            sock.sendall(chunk)
        ack = sock.recv(1)
        t1 = time.time()
        elapsed = t1 - t0
    finally:
        sock.close()
    result = {"role": "send", "size_bytes": size, "elapsed_s": round(elapsed, 3),
              "t0": round(t0, 6), "t1": round(t1, 6), "probe_wait_s": waited,
              "sender_MiB_per_s": round(size / elapsed / 1048576, 3) if elapsed else None,
              "ack": ack.decode(errors="replace"), "sha_expected": expected(size)}
    write_json(out, result)
    print(json.dumps(result), flush=True)


def probe(target, count, interval_ms, out, timeout, go_file=None, stop_file=None,
          max_seconds=600.0, ready_file=None, bulk_json=None):
    """Sample 64-byte round trips and keep only the ones taken while the bulk flow ran.

    The window is what makes the number mean something. The probe connects first and writes a
    readiness file, which is what the sender waits for before its first byte; the sender then
    records the absolute interval it spent streaming, and every sample below is filtered to
    that interval. A sample that fell before the transfer started, or after it finished, was
    taken on an idle path and is not counted.
    """
    samples, errors = [], 0
    waited = 0.0
    t_start = time.time()
    sock = connect(target, timeout)
    try:
        sock.sendall(b"E")
        if ready_file:
            # The echo stream exists and the sink has accepted it: the sender may go.
            open(ready_file, "w").close()
        if go_file:
            while not os.path.exists(go_file):
                if time.time() - t_start > 120:
                    break
                time.sleep(0.01)
            waited = round(time.time() - t_start, 3)
        payload = bytes(range(ECHO))
        for _ in range(count):
            if stop_file and os.path.exists(stop_file) and samples:
                break
            if time.time() - t_start > max_seconds:
                break
            t0 = time.perf_counter()
            sock.sendall(payload)
            got = b""
            while len(got) < ECHO:
                part = sock.recv(ECHO - len(got))
                if not part:
                    raise ConnectionError("eof")
                got += part
            samples.append((time.time(), (time.perf_counter() - t0) * 1e6))
            time.sleep(interval_ms / 1000.0)
    except Exception:  # noqa: BLE001 - counted, and the sample list is what it is
        errors += 1
    finally:
        sock.close()

    window = None
    if bulk_json:
        try:
            bulk = json.load(open(bulk_json))
            t0, t1 = bulk.get("t0"), bulk.get("t1")
            if t0 and t1 and t1 > t0:
                window = (t0, t1)
        except Exception:  # noqa: BLE001 - no window file means no filtering is possible
            window = None
    # No window means the sender never recorded one: keep everything and let the stage judge,
    # because filtering is only meaningful against the interval this transfer really held.
    kept = [us for ts, us in samples if window is None or window[0] <= ts <= window[1]]
    result = {"role": "probe", "count": len(kept), "ok": len(kept), "errors": errors,
              "samples_total": len(samples), "bulk_window": window is not None,
              "go_wait_s": waited, "window_s": round(time.time() - t_start, 3),
              "p50_us": round(pct(kept, 0.50), 1) if kept else None,
              "p95_us": round(pct(kept, 0.95), 1) if kept else None,
              "p99_us": round(pct(kept, 0.99), 1) if kept else None,
              "max_us": round(max(kept), 1) if kept else None}
    write_json(out, result)
    print(json.dumps(result), flush=True)


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("role", choices=["server", "send", "probe"])
    ap.add_argument("--port", type=int)
    ap.add_argument("--bulk-size", type=int)
    ap.add_argument("--target")
    ap.add_argument("--size", type=int)
    ap.add_argument("--count", type=int, default=20000, help="cap on samples")
    ap.add_argument("--interval-ms", type=float, default=50)
    ap.add_argument("--go-file")
    ap.add_argument("--start-file")
    ap.add_argument("--wait-file")
    ap.add_argument("--ready-file")
    ap.add_argument("--bulk-json")
    ap.add_argument("--stop-file")
    ap.add_argument("--max-seconds", type=float, default=600)
    ap.add_argument("--idle", type=float, default=45)
    ap.add_argument("--timeout", type=float, default=60)
    ap.add_argument("--json", required=True)
    a = ap.parse_args(argv)
    try:
        if a.role == "server":
            if not a.port or not a.bulk_size:
                ap.error("server needs --port and --bulk-size")
            server(a.port, a.bulk_size, a.json, a.idle)
        elif a.role == "send":
            if not a.target or not a.size:
                ap.error("send needs --target and --size")
            send(a.target, a.size, a.json, a.timeout, a.start_file, a.wait_file)
        else:
            if not a.target:
                ap.error("probe needs --target")
            probe(a.target, a.count, a.interval_ms, a.json, a.timeout, a.go_file, a.stop_file,
                  a.max_seconds, a.ready_file, a.bulk_json)
    except Exception as exc:  # noqa: BLE001 - the record has to exist for the stage to read it
        # A missing file and a failed flow look the same to the stage; write the failure instead.
        write_json(a.json, {"role": a.role, "error": type(exc).__name__, "failed": True})
        raise


if __name__ == "__main__":
    main(sys.argv[1:])
