#!/usr/bin/env python3
"""BA 探测：mcode acp 的完整 JSON-RPC 往返。
initialize → session/new → session/prompt。
上次用 shell 管道没法回填 sessionId，这次用真正的驱动进程。
"""
import json, subprocess, sys, threading, time

proc = subprocess.Popen(
    ["mcode", "acp"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    text=True, bufsize=1,
)

def send(obj):
    proc.stdin.write(json.dumps(obj) + "\n")
    proc.stdin.flush()

def read_until(pred, timeout=180):
    """读行直到 pred(line) 为真或超时。"""
    box = {}
    def worker():
        for line in proc.stdout:
            line = line.strip()
            if not line:
                continue
            box["line"] = line
            if pred(line):
                return
    t = threading.Thread(target=worker, daemon=True)
    t.start()
    t.join(timeout)
    return box.get("line")

# ── 1. initialize ──
send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": {"protocolVersion": 1,
                 "clientCapabilities": {"fs": {"readTextFile": False, "writeTextFile": False}}}})
line = read_until(lambda l: '"id":1' in l or '"id": 1' in l, 60)
print("=== 1 initialize ===")
print(line[:400] if line else "(超时,无响应)")
if not line:
    proc.kill(); sys.exit(1)
r = json.loads(line)
if "error" in r:
    print("initialize 失败:", r["error"]); proc.kill(); sys.exit(1)

# ── 2. session/new ──
send({"jsonrpc": "2.0", "id": 2, "method": "session/new",
      "params": {"cwd": "/tmp", "mcpServers": []}})
line = read_until(lambda l: '"id":2' in l or '"id": 2' in l, 90)
print("\n=== 2 session/new ===")
if not line:
    print("(超时)"); proc.kill(); sys.exit(1)
r2 = json.loads(line)
if "error" in r2:
    print("session/new 失败:", r2["error"]); proc.kill(); sys.exit(1)
sid = r2["result"]["sessionId"]
print("sessionId =", sid)
print("agentInfo =", json.dumps(r2["result"].get("agentInfo"), ensure_ascii=False))
print("capabilities 键 =", sorted(r2["result"].get("capabilities", {}).keys()) if "capabilities" in r2["result"] else sorted(r2["result"].keys()))

# ── 3. session/prompt ──
send({"jsonrpc": "2.0", "id": 3, "method": "session/prompt",
      "params": {"sessionId": sid,
                 "prompt": [{"type": "text", "text": "只回两个字：收到"}]}})
print("\n=== 3 session/prompt（首次回复可能较慢）===")
t0 = time.time()
lines = []
deadline = time.time() + 180
while time.time() < deadline:
    l = read_until(lambda x: True, 180)
    if not l:
        break
    lines.append(l)
    if '"id":3' in l or '"id": 3' in l:
        break
el = time.time() - t0
print(f"耗时 {el:.1f}s，共收到 {len(lines)} 条")
for l in lines[:12]:
    print("  ", l[:260])
if any('"id":3' in l or '"id": 3' in l for l in lines):
    final = [l for l in lines if '"id":3' in l or '"id": 3' in l][-1]
    print("\n=== 3 结果 ===")
    print(final[:800])
else:
    print("\n⚠️ 未收到 id=3 的最终响应")

proc.kill()