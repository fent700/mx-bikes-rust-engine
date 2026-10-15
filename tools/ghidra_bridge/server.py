#!/usr/bin/env python3
"""
Ghidra MCP bridge for Claude Code.

Runs Ghidra headless analysis (scripts/DumpProgram.java) once per binary and serves the exported
program database over MCP stdio: functions, decompiled C, strings with cross references and
float/double constants referenced by code.

Environment:
  GHIDRA_HOME       Ghidra install (default N:/programs/ghidra_12.1.4_PUBLIC)
  GHIDRA_PROJECTS   project + dump folder (default N:/programs/ghidra_projects)
"""

import json
import math
import os
import re
import subprocess
from collections import Counter
from pathlib import Path

from mcp.server.fastmcp import FastMCP

GHIDRA_HOME = Path(os.environ.get("GHIDRA_HOME", r"N:\programs\ghidra_12.1.4_PUBLIC"))
PROJECTS = Path(os.environ.get("GHIDRA_PROJECTS", r"N:\programs\ghidra_projects"))
SCRIPTS = Path(__file__).resolve().parent / "scripts"

mcp = FastMCP("ghidra")


class Dump:
    """Lazily loaded JSONL export of one analysed program."""

    def __init__(self, folder: Path):
        self.folder = folder
        self.functions = {}
        self.by_name = {}
        self.strings = []
        self.data_xrefs = []
        self.loaded = False

    def load(self):
        if self.loaded:
            return
        fpath = self.folder / "functions.jsonl"
        if not fpath.exists():
            raise FileNotFoundError(f"no analysis in {self.folder}; call run_analysis first")
        with open(fpath, encoding="utf-8") as f:
            for line in f:
                rec = json.loads(line)
                self.functions[rec["addr"].lower()] = rec
                self.by_name.setdefault(rec["name"], rec)
        for name, target in (("strings.jsonl", self.strings), ("data_xrefs.jsonl", self.data_xrefs)):
            p = self.folder / name
            if p.exists():
                with open(p, encoding="utf-8") as f:
                    target.extend(json.loads(line) for line in f)
        self.loaded = True

    def find(self, name_or_addr: str):
        self.load()
        key = name_or_addr.strip().lower().removeprefix("0x")
        for k in (key, key.lstrip("0")):
            if k in self.functions:
                return self.functions[k]
        if name_or_addr in self.by_name:
            return self.by_name[name_or_addr]
        try:
            target = int(key, 16)
        except ValueError:
            return None
        best = None
        for addr, rec in self.functions.items():
            start = int(addr, 16)
            if start <= target < start + max(rec.get("size", 1), 1) and (best is None or start > int(best["addr"], 16)):
                best = rec
        return best


_dumps: dict[str, Dump] = {}


def dump_for(program: str) -> Dump:
    folder = PROJECTS / f"{program}_dump"
    if program not in _dumps:
        _dumps[program] = Dump(folder)
    return _dumps[program]


def section_entropy(binary: Path) -> dict:
    data = binary.read_bytes()
    pe = int.from_bytes(data[0x3C:0x40], "little")
    nsec = int.from_bytes(data[pe + 6:pe + 8], "little")
    opt = int.from_bytes(data[pe + 20:pe + 22], "little")
    out = {}
    for i in range(nsec):
        s = pe + 24 + opt + i * 40
        name = data[s:s + 8].rstrip(b"\0").decode(errors="replace")
        size = int.from_bytes(data[s + 16:s + 20], "little")
        raw = int.from_bytes(data[s + 20:s + 24], "little")
        blk = data[raw:raw + size]
        if not blk:
            continue
        c = Counter(blk)
        out[name] = round(-sum(v / len(blk) * math.log2(v / len(blk)) for v in c.values()), 3)
    return out


@mcp.tool()
def check_binary(binary_path: str) -> str:
    """Reports PE section entropy and whether the code section looks encrypted (e.g. SteamStub)."""
    p = Path(binary_path)
    ent = section_entropy(p)
    packed = ent.get(".text", 0) > 7.9
    if packed:
        verdict = "code section is encrypted/packed: decompilation will be noise until an unwrapped copy is analysed"
    elif ".bind" in ent:
        verdict = "code section is plain machine code (SteamStub .bind kept but .text already decrypted)"
    else:
        verdict = "code section looks like plain machine code"
    return json.dumps({"sections": ent, "packed": packed, "verdict": verdict}, indent=2)


@mcp.tool()
def run_analysis(binary_path: str, program: str = "", timeout_minutes: int = 120) -> str:
    """Imports and auto-analyses a binary with Ghidra headless, then exports it for querying."""
    binary = Path(binary_path)
    program = program or binary.stem
    out = PROJECTS / f"{program}_dump"
    out.mkdir(parents=True, exist_ok=True)
    launcher = GHIDRA_HOME / "support" / ("analyzeHeadless.bat" if os.name == "nt" else "analyzeHeadless")
    cmd = [
        str(launcher), str(PROJECTS), program, "-import", str(binary), "-overwrite",
        "-scriptPath", str(SCRIPTS), "-postScript", "DumpProgram.java", str(out),
    ]
    log = PROJECTS / f"{program}_analyze.log"
    with open(log, "w", encoding="utf-8") as f:
        proc = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT, timeout=timeout_minutes * 60)
    _dumps.pop(program, None)
    tail = log.read_text(encoding="utf-8", errors="replace").splitlines()[-15:]
    return json.dumps({"exit_code": proc.returncode, "dump": str(out), "log_tail": tail}, indent=2)


@mcp.tool()
def list_functions(program: str = "mxbikes", name_filter: str = "", offset: int = 0, limit: int = 100) -> str:
    """Lists analysed functions (address, name, size), optionally filtered by a name regex."""
    d = dump_for(program)
    d.load()
    rx = re.compile(name_filter, re.I) if name_filter else None
    rows = [
        {"addr": r["addr"], "name": r["name"], "size": r["size"]}
        for r in d.functions.values()
        if rx is None or rx.search(r["name"])
    ]
    return json.dumps({"total": len(rows), "functions": rows[offset:offset + limit]}, indent=2)


@mcp.tool()
def decompile_function(name_or_address: str, program: str = "mxbikes") -> str:
    """Returns Ghidra's decompiled C for a function by name, entry address or contained address."""
    rec = dump_for(program).find(name_or_address)
    if rec is None:
        return f"no function matches {name_or_address}"
    return f"// {rec['name']} @ {rec['addr']} (callers: {len(rec['callers'])}, callees: {len(rec['callees'])})\n{rec['c']}"


@mcp.tool()
def function_xrefs(name_or_address: str, program: str = "mxbikes") -> str:
    """Callers and callees of a function."""
    rec = dump_for(program).find(name_or_address)
    if rec is None:
        return f"no function matches {name_or_address}"
    d = dump_for(program)
    named = lambda addrs: [{"addr": a, "name": d.functions.get(a.lower(), {}).get("name", "?")} for a in addrs]
    return json.dumps({"function": rec["name"], "callers": named(rec["callers"]), "callees": named(rec["callees"])}, indent=2)


@mcp.tool()
def search_strings(pattern: str, program: str = "mxbikes", limit: int = 50) -> str:
    """Finds defined strings matching a regex and the functions that reference them."""
    d = dump_for(program)
    d.load()
    rx = re.compile(pattern, re.I)
    hits = []
    for s in d.strings:
        if rx.search(s["value"]):
            hits.append({
                "addr": s["addr"],
                "value": s["value"],
                "referenced_by": [{"addr": a, "name": d.functions.get(a.lower(), {}).get("name", "?")} for a in s["xrefs"]],
            })
            if len(hits) >= limit:
                break
    return json.dumps(hits, indent=2)


@mcp.tool()
def search_decompiled(pattern: str, program: str = "mxbikes", limit: int = 20) -> str:
    """Regex search over all decompiled functions; returns matching lines with function names."""
    d = dump_for(program)
    d.load()
    rx = re.compile(pattern)
    out = []
    for r in d.functions.values():
        lines = [ln.strip() for ln in r["c"].splitlines() if rx.search(ln)]
        if lines:
            out.append({"addr": r["addr"], "name": r["name"], "lines": lines[:8]})
            if len(out) >= limit:
                break
    return json.dumps(out, indent=2)


@mcp.tool()
def constant_users(value: float, program: str = "mxbikes", tolerance: float = 1e-4) -> str:
    """Functions referencing a float/double constant (useful for locating physics code)."""
    d = dump_for(program)
    d.load()
    out = []
    for rec in d.data_xrefs:
        try:
            v = float(rec["value"])
        except ValueError:
            continue
        if abs(v - value) <= tolerance * max(1.0, abs(value)):
            out.append({"addr": rec["addr"], "type": rec["type"], "functions": rec["func_addrs"]})
    return json.dumps(out, indent=2)


if __name__ == "__main__":
    mcp.run()
