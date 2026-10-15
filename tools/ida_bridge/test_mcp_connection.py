#!/usr/bin/env python3
"""
Tests connection to IDA Pro MCP server and JSON-RPC bridge.
"""

import http.client
import json
import sys

def test_ida_rpc(host="127.0.0.1", port=13337):
    print(f"Connecting to IDA Pro RPC plugin at http://{host}:{port}/mcp...")
    try:
        conn = http.client.HTTPConnection(host, port, timeout=3.0)
        req = {
            "jsonrpc": "2.0",
            "method": "check_connection",
            "params": [],
            "id": 1,
        }
        conn.request("POST", "/mcp", json.dumps(req), {"Content-Type": "application/json"})
        resp = conn.getresponse()
        data = json.loads(resp.read().decode())
        print("Response received from IDA Pro:", data)
        if "result" in data:
            print(">>> IDA Pro MCP plugin is LIVE and responsive!")
            return True
        else:
            print(">>> Unexpected RPC response:", data)
            return False
    except ConnectionRefusedError:
        print(">>> IDA Pro is currently not running with the MCP plugin active.")
        print(">>> To activate: Launch IDA Pro GUI with mxbikes.exe loaded.")
        print(">>> The mcp-plugin.py automatically starts the RPC server on port 13337.")
        return False
    except Exception as e:
        print(f">>> Connection check error: {e}")
        return False

if __name__ == "__main__":
    test_ida_rpc()
