#!/usr/bin/env python3
"""
Diagnostic tool to inspect and verify MX Bikes installation and reverse-engineering toolchain.
"""

import sys
import os
import glob
import zipfile
from pathlib import Path

GAME_DIR = Path(r"N:\SteamLibrary\steamapps\common\MX Bikes")
IDA_DIR = Path(r"N:\programs\ida")

def main():
    print("=" * 60)
    print("  OPEN-MXBIKES REWRITE ENVIRONMENT DIAGNOSTIC")
    print("=" * 60)

    # 1. Verify Game Installation
    print(f"\n[1] Checking Game Directory: {GAME_DIR}")
    if not GAME_DIR.exists():
        print(f"  [ERROR] Game directory does not exist!")
        return 1
    
    exe_path = GAME_DIR / "mxbikes.exe"
    if exe_path.exists():
        size_mb = exe_path.stat().st_size / (1024 * 1024)
        print(f"  [OK] mxbikes.exe found ({size_mb:.2f} MB)")
    else:
        print(f"  [ERROR] mxbikes.exe missing!")

    # 2. Check PKZ Archives
    print("\n[2] Checking PKZ Asset Archives:")
    pkz_files = list(GAME_DIR.glob("*.pkz"))
    total_entries = 0
    for pkz in sorted(pkz_files):
        try:
            with zipfile.ZipFile(pkz, "r") as z:
                count = len(z.namelist())
                total_entries += count
                size_mb = pkz.stat().st_size / (1024 * 1024)
                print(f"  - {pkz.name:15} {size_mb:8.2f} MB ({count:4d} assets)")
        except Exception as e:
            print(f"  - {pkz.name:15} [ERROR: {e}]")

    print(f"  Total Indexed Assets: {total_entries}")

    # 3. Check IDA Pro Environment
    print(f"\n[3] Checking IDA Pro Installation: {IDA_DIR}")
    if (IDA_DIR / "ida.exe").exists():
        print(f"  [OK] ida.exe found")
    if (IDA_DIR / "idat.exe").exists():
        print(f"  [OK] idat.exe (headless) found")
    if (IDA_DIR / "idalib.dll").exists():
        print(f"  [OK] idalib.dll found (IDA 9.x SDK)")

    # 4. Check MCP Bridge for Claude Code
    print("\n[4] Checking MCP Bridge Configuration:")
    claude_config = Path(os.environ.get("USERPROFILE", "")) / ".claude.json"
    if claude_config.exists():
        print(f"  [OK] Claude Code config found at {claude_config}")
    
    ida_plugin = Path(os.environ.get("APPDATA", "")) / "Hex-Rays" / "IDA Pro" / "plugins" / "mcp-plugin.py"
    if ida_plugin.exists():
        print(f"  [OK] IDA Pro MCP plugin installed at {ida_plugin}")
    else:
        print(f"  [WARN] IDA Pro MCP plugin not installed at default location")

    # 5. Check OpenBin CLI
    openbin_exe = Path(os.environ.get("LOCALAPPDATA", "")) / "Programs" / "openbin" / "openbin.exe"
    if openbin_exe.exists():
        print(f"  [OK] openbin CLI installed at {openbin_exe}")
    else:
        print(f"  [WARN] openbin CLI not found")

    print("\nEnvironment is ready for open-source development.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
