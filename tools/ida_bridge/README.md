# IDA Pro MCP Bridge for Claude Code

This directory documents the MCP (Model Context Protocol) integration bridging **IDA Pro 9.3** and **Claude Code** for the MX Bikes open-source engine rewrite.

---

## Architecture Overview

```
 ┌──────────────────────┐         HTTP (JSON-RPC)         ┌─────────────────────────┐
 │   IDA Pro 9.3 GUI    │ ◄─────────────────────────────► │   ida_pro_mcp Server    │
 │ (mcp-plugin.py:13337)│                                 │       (Python 3.12)     │
 └──────────────────────┘                                 └────────────┬────────────┘
                                                                       │ stdio (MCP)
                                                                       ▼
                                                          ┌─────────────────────────┐
                                                          │       Claude Code       │
                                                          │ (agent / terminal CLI)  │
                                                          └─────────────────────────┘
```

1. **IDA Plugin**: Located at `%APPDATA%\Hex-Rays\IDA Pro\plugins\mcp-plugin.py`. Starts a local JSON-RPC server on port `13337` when IDA Pro is launched.
2. **MCP Bridge Server**: `ida_pro_mcp` running on Python 3.12, translating MCP tool calls to JSON-RPC.
3. **Claude Code**: Configured globally in `~/.claude.json` and in `.mcp.json`.

---

## MCP Tools Available to Claude Code

When working with Claude Code, the following tools are directly available:

| Tool | Description |
|---|---|
| `check_connection` | Confirms the IDA Pro plugin is listening and responsive. |
| `list_functions` | Enumerates functions in the database with names and addresses. |
| `get_function_by_name` | Retrieves assembly or metadata for a named function. |
| `get_function_by_address` | Retrieves a function by its hexadecimal virtual address. |
| `decompile_function` | Runs Hex-Rays decompiler and returns structured C pseudocode. |
| `disassemble_function` | Returns assembly listing for a function. |
| `list_strings` | Searches ASCII and Unicode strings across the binary. |
| `get_xrefs_to` | Finds cross-references to functions, variables, or strings. |
| `set_function_prototype` | Updates function signature in the database. |
| `rename_function` | Renames an unknown `sub_...` to a descriptive name. |
| `add_comment` | Leaves notes in the IDB for collaborative reversing. |

---

## How to Use with Claude Code

1. Start **IDA Pro** and open `N:\SteamLibrary\steamapps\common\MX Bikes\mxbikes.exe`.
2. Wait for auto-analysis to finish. The Output window will display:
   ```
   [MCP] Server started on http://127.0.0.1:13337/mcp
   ```
3. Run `claude` in `N:\etc\rewrite project`:
   ```bash
   claude
   ```
4. Ask Claude Code to inspect functions directly:
   > *"Claude, check the IDA connection, find the function that references the string 'warning: destroying world containing grouped joints', decompile it, and tell me how the ODE physics world is initialized."*
