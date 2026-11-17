#!/usr/bin/env python3
"""Tunnel benchmark/integrity harness (E1 loopback; stdlib only).
Sends N bytes of deterministic data through TARGET (host:port that reaches the sink),
optionally cutting the path mid-transfer via a local cutter proxy, and compares SHA-256.
Usage: harness.py --size-mib 256 [--target 127.0.0.1:PORT] [--cut-at-mib 64] [--cut-s 2] [--json out]
Without --target a plain relay is used as a NULL CONTROL (validates the harness itself).
Reports: sha_match, bytes_received, throughput, duration, fault timeline, rss/cpu of the harness.
"""
import argparse, hashlib, json, os, resource, socket, threading, time

CH = 1 << 16

def gen(n, seed=b"baft"):
    """deterministic stream; returns generator of chunks"""
    i, sent = 0, 0
    while sent < n:
        blk = hashlib.sha256(seed + i.to_bytes(8, "big")).digest() * (CH // 32)
        blk = blk[: min(CH, n - sent)]
        sent += len(blk); i += 1
        yield blk

def expected(n):
    h = hashlib.sha256()
    for b in gen(n): h.update(b)
    return h.hexdigest()

def sink(port, res, ready):
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", port)); s.listen(8); ready.set()
    h, n, t0 = hashlib.sha256(), 0, None
    while True:
        c, _ = s.accept()
        t0 = t0 or time.time()
        while True:
            d = c.recv(CH)
            if not d: break
            h.update(d); n += len(d)
            res["n"], res["sha"], res["t1"] = n, h.hexdigest(), time.time()
        c.close()
        if n >= res["want"]: break
    res["t0"] = t0

def relay(lport, tport, cut_at, cut_s, log):
    """null-control proxy, optional cut: closes connection after cut_at bytes, refuses for cut_s"""
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", lport)); s.listen(8)
    down_until = [0]
    def one(c):
        u = socket.create_connection(("127.0.0.1", tport)); n = 0
        while True:
            d = c.recv(CH)
            if not d: break
            if cut_at and n < cut_at <= n + len(d):
                log.append(("cut", time.time())); down_until[0] = time.time() + cut_s
                break
            u.sendall(d); n += len(d)
        u.close(); c.close()
    while True:
        c, _ = s.accept()
        if time.time() < down_until[0]: c.close(); continue
        threading.Thread(target=one, args=(c,), daemon=True).start()

def send(addr, size, cut_s_retry, log):
    """sends the stream; on reset reconnects and CONTINUES from the failed offset (app-level retry, reported)."""
    sent, retries = 0, 0; it = gen(size); pend = b""
    while sent < size:
        try:
            c = socket.create_connection(addr, timeout=10)
            while sent < size:
                if not pend: pend = next(it)
                c.sendall(pend); sent += len(pend); pend = b""
            c.close()
        except OSError as e:
            retries += 1; log.append(("send_error", time.time(), str(e)[:60])); time.sleep(0.5)
            if retries > 20: break
    return sent, retries

if __name__ == "__main__":
    a = argparse.ArgumentParser()
    a.add_argument("--size-mib", type=int, default=64); a.add_argument("--target")
    a.add_argument("--cut-at-mib", type=int, default=0); a.add_argument("--cut-s", type=float, default=2)
    a.add_argument("--json"); a = a.parse_args()
    size = a.size_mib << 20; log = []; res = {"want": size}; ready = threading.Event()
    sp = 39001; threading.Thread(target=sink, args=(sp, res, ready), daemon=True).start(); ready.wait()
    if a.target:
        host, port = a.target.rsplit(":", 1); addr = (host, int(port)); mode = "target"
    else:
        threading.Thread(target=relay, args=(39002, sp, a.cut_at_mib << 20, a.cut_s, log), daemon=True).start()
        time.sleep(0.2); addr = ("127.0.0.1", 39002); mode = "null-control"
    t0 = time.time(); sent, retries = send(addr, size, a.cut_s, log); time.sleep(1)
    dur = time.time() - t0; r = resource.getrusage(resource.RUSAGE_SELF)
    out = {"env": "E1-loopback", "mode": mode, "size_bytes": size, "sent": sent,
           "received": res.get("n", 0), "sha_expected_of_full_stream": expected(size),
           "sha_received": res.get("sha"), "retries": retries, "app_level_retry_used": retries > 0,
           "duration_s": round(dur, 3), "MiB_per_s": round(res.get("n", 0) / dur / 1048576, 1),
           "faults": [(x[0], round(x[1] - t0, 2)) for x in log],
           "harness_cpu_s": round(r.ru_utime + r.ru_stime, 2), "harness_maxrss_MiB": r.ru_maxrss // 1024, "host": os.uname().nodename}
    out["sha_match"] = out["sha_expected_of_full_stream"] == out["sha_received"]
    out["verdict"] = "correct" if out["sha_match"] else ("truncated" if out["received"] < size else "corrupt")
    print(json.dumps(out, indent=1))
    if a.json: open(a.json, "w").write(json.dumps(out, indent=1))
