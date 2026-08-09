import json
import sys
import time

mode = sys.argv[1]
if mode == "version":
    print("fake-provider 1.2.3")
elif mode == "old-version":
    print("fake-provider 0.1.0")
elif mode == "echo":
    sys.stdout.write(sys.stdin.read())
elif mode == "graphify":
    print("Graph evidence: module A calls module B")
elif mode == "sleep":
    time.sleep(2)
elif mode == "crash":
    raise SystemExit(7)
elif mode == "large":
    sys.stdout.write("x" * 4096)
elif mode.startswith("mcp"):
    for line in sys.stdin:
        request = json.loads(line)
        if "id" not in request:
            continue
        method = request.get("method")
        if method == "initialize":
            result = {
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {}},
                "serverInfo": {
                    "name": "fake-provider",
                    "version": "0.34.0" if mode == "mcp-headroom" else "1.2.3",
                },
            }
        elif method == "tools/list":
            result = {"tools": [{
                "name": name,
                "description": "contract test",
                "inputSchema": {"type": "object"},
            } for name in ("fake_search", "search", "headroom_compress")]}
        elif method == "tools/call":
            tool = request.get("params", {}).get("name")
            payload = (
                {"compressed": "short", "original_tokens": 10, "compressed_tokens": 2}
                if tool == "headroom_compress"
                else {"records": [{"id": "one", "content": "safe result"}]}
            )
            result = {"content": [{"type": "text", "text": json.dumps(payload)}]}
        else:
            result = {}
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
else:
    raise SystemExit(2)
