# Ghidra MCP bridge

An MCP server exposing a Ghidra-analysed binary to Claude Code. Use it instead of the IDA
bridge when IDA cannot start; the last IDA launch failed with "Cannot continue without a valid
license".

```
 analyzeHeadless + scripts/DumpProgram.java  ──>  N:/programs/ghidra_projects/<program>_dump/*.jsonl
                                                        │
                                   server.py (stdio MCP) ┘  <── Claude Code (.mcp.json "ghidra")
```

## Tools

| Tool | Purpose |
|---|---|
| `check_binary` | PE section entropy; detects encrypted/packed code (SteamStub) |
| `run_analysis` | headless import + auto-analysis + export (takes minutes) |
| `list_functions` | functions with optional name regex |
| `decompile_function` | decompiled C by name, entry or contained address |
| `function_xrefs` | callers / callees |
| `search_strings` | strings by regex, with referencing functions |
| `search_decompiled` | regex over all decompiled code |
| `constant_users` | functions referencing a float/double constant |

Ghidra 12.1.4 is installed at `N:\programs\ghidra_12.1.4_PUBLIC` and runs on the system JDK 26.

## Stock `mxbikes.exe` is SteamStub-wrapped

`check_binary` reports `.text` entropy 8.00 and a `.bind` section: the code is encrypted on disk
and only decrypted by the SteamStub loader at runtime. Ghidra or IDA output for the stock exe is
noise. `.rdata` is not encrypted, so string and constant tables are readable, but they have no
code cross references.

An unwrapped copy was produced from the user's own install with Steamless 3.1.0.5
(`N:\programs\mxbikes_re\mxbikes.exe.unpacked.exe`, SteamStub Variant 3.1 x64; `.text` entropy
6.41) and analysed into `N:\programs\ghidra_projects\mxbikes_dump` (4903 functions). The Steam
install itself is untouched. Neither the unwrapped exe nor the decompiled output belongs in this
repository.

Findings so far:

| Topic | Location | Result |
|---|---|---|
| Steering gains loader | `FUN_140168cd0` | `Steer/*` keys -> bike struct `0x36f8..0x39d8` |
| Rider steering controller | `FUN_14019d360` | PID on lean error (deg) with `spg/sdg/sig` scheduled by speed, `KYaw` yaw-rate term, front-load fade, +-250 N·m, ODE AMotor torque |
| Surface enumeration | `FUN_14018e8f0` | 0 asphalt1 .. 13 rock (see FORMATS.md) |
| Bike tyre Magic Formula | loader `FUN_14018e8f0`, evaluator `FUN_1401e2a90` -> `FUN_1401e2890` (combined) -> `FUN_1401e2500` (Fy0) | MF-MC lateral terms and the `q_sy1` slip shift (see FORMATS.md). `FUN_1401c02e0` / `FUN_1401e1450` are the `type = car` tyre path and do not apply to bikes |
| Skinned meshes (EDF) | `FUN_140213a70` | Node flag `0x2000`: 6-word submesh records + vertex count + 124 B/vertex skinning weight blocks |
| EDF morph sets | `FUN_140213a70` | Node flag `0x10000`: `count {frames, pos, normal, tangent, time[frames]}`; the node vertex-count pair is always read |
| Material record | `FUN_14020ff30` (skip variant `FUN_140210f10`) | Reflection/specular maps carry frame lists when shading == 2; paint layers are `count {56 B header, material}` |
| Legacy map (`MAP\0`) | `FUN_140212620` (falls back to `FUN_140212390` for `MP2\0`) | Menu background `misc/bg/bg.map` |
| Skeletal animation (.anm) | data files | Layout decoded and checked from the 92 stock files (see FORMATS.md). An earlier "opcode `0x81`" note was never confirmed in the exe and has been dropped |
| Track spawn / stall layout | `FUN_1401027b0`, centreline `FUN_1401f3eb0` / `FUN_1401f1060` / `FUN_1401f23f0` | `pit_lane/start_stall%d/{long,lat,angle}` mapped through the TRH centreline. Heading = line heading + stall angle |
| Rider animation update | `FUN_1400523e0` | Lean sets at phase `(1 - clamp(lean/maxlean, -1, 1)) / 2` (lean folded past 90 deg); weights from fore/aft, standing, airborne and ground-sit; head-lean set alike; throttle layer at the throttle position. Bike lean is `FUN_14019d360` field `+0x3878` = `atan2(up.x, up.y)` in degrees |
| Pose evaluator | `FUN_14021fdd0` (setters `FUN_14021d130/d210/d2c0/d330` = enable / phase / weight / layer) | Layers highest first, lower layers get the remainder; running weighted slerp with `clip weight * per-bone .anm mask`; the first clip on a joint always lands |
| Rider body state | `FUN_1401a8480` | Torso lean, standing and fore/aft states, rate-limited by values that appear to come from `rider.prm` (not decoded) |
| Bitmap fonts | `FUN_140208e70` | `.fnt` layout (see FORMATS.md) |
| Roost particles | loader `FUN_14004e450`, setup `FUN_14004ebc0`, per-wheel emission `FUN_140050480`, engine `FUN_140129360` / `FUN_1401293e0` / `FUN_140247d50` | Effect per surface, track tints, exit velocity, size factor, `texture_slots` = log2 cells |
| Dirt / mud | accumulation `FUN_1401a8480`, stage `FUN_1400523e0`, frame select `FUN_14021f970` | +0.0001 per metre on grass/soil/soft/compact; stages at 1/6, 1/2, 5/6 select mud frames |
| Low-speed steering gains | `FUN_140197060` | Kp = -600 * steering inertia, Kd = 0.05 Kp; used below 1 m/s in `FUN_14019d360` |
| Rider aids | `FUN_1400d5fa0` | `profile.ini` keys `autoshift`, `autoclutch`, `leanhelp`, `shifthelp`, `brakehelp`, `tractionhelp` |

All asset formats in `docs/FORMATS.md` were decoded from the data files alone and do not depend
on this.

