# START HERE (handoff for a new agent, 2026-10-12)

You are picking up **Open-MXBikes**: a clean-room Rust / Bevy 0.15 reimplementation of the game
MX Bikes that runs on the game's own assets. Goal: physics, graphics, functions and UI 1:1 with
the original. Read this section fully, then the rest of this file as reference.

## Version control (2026-10-14)

The repo is now a local git repository. Commit after each working change and read `git diff`
before trusting another agent's report. Fixed on 2026-10-14 after review: an invented crash rule
(chassis force limits taken from the rider-body spring check of `FUN_1401a8480`) that crashed the
bike on every landing; the helmet camera (now at the helmet `cam` object, helmet hidden, pitched
down 15 + 20 * view/pitch degrees, `FUN_14000b7e0`); the program now starts at the main menu;
profile `gfx` x / y default to 0 (desktop size), not 800 x 600. Autoshift was checked and works at
full throttle (`examples/autoshift_run.rs`); it does not shift at part throttle below the table rpm,
as in the game.

### Tyre, wheel slip, aids, airborne steering (2026-10-14, later)

- `tyre_model.rs` is now a term-for-term port of the bike tyre evaluator `FUN_1401e2a90` with
  `FUN_1401e2500` (pure lateral), `FUN_1401e2890` (combined lateral) and `FUN_1401e21d0` (slip
  stiffness). The packed array is at tyre + 0xc0. Per step the wheel code (`FUN_1401a2fb0` ->
  `FUN_1401b92d0`) writes slot 0x11 / 0x2b = surface grip, 0x12 / 0x2c = `long_stiffness` /
  `lat_stiffness` (1 when absent), 0x63 = rolling resistance (`resistance_dry`/`_wet`).
  Fixed against the previous version: the camber residual moment is `Fz R0 q_Dz8 gamma cos(alpha)
  * grip` (the old code had an invented `* 0.1` and a `Gxa` factor); the trail uses the combined
  equivalent slip `sqrt(alpha^2 + (Kx / (Ky + 0.1))^2 kappa^2)` and `cos(alpha)`; `Gyk`/`Gxa` are
  not clamped at zero; B denominators are `C D + 0.1`; rolling resistance is the moment
  `My = -R0 Fz res (q_sy1 atan(v_roll / 16.667) + q_sy2 Fx / Fz0)` applied to the wheel spin
  (dBodyAddRelTorque), replacing the invented `Fz * res * 0.01` force. Antigravity's q_sy1 slip
  shift was correct.
- Wheel slip states follow `FUN_1401a2fb0`: relaxation length 0.15 m for both, state clamps
  +-0.5 / +-1, low-speed terms `(cos(pi |vx| / 2.5) + 1) * 500 / K_eff * v_slip` below 2.5 m/s
  (replacing the invented creep damping), effective rolling radius `R1 - (R1 - loaded) / 3`,
  tyre force applied at the ground contact and the road torque using the loaded radius.
- Brakes (`FUN_1401a5780`): dead zone = leverage ratio * DeadZone. Max pressure formula checked
  (`FUN_140196500`). The disc temperature gaussian uses OptTemp * TempFactor as sigma, which is
  ~1 for stock discs, so it is not modelled.
- `brakehelp` and `tractionhelp` are now the game's (`FUN_1401ada40`, ops 0x266 / 0x267 set
  bike + 0x23960 / 0x23988). Brake help, above 8 m/s: front released when its slip passes
  `-cos^2(lean) * kappa_peak`, front limited by the front load share (`2 Ff/(Ff+Fr) - 1` against
  `0.7 + 0.3 cos^200(lean)`, 0.4 ramp), rear released on lock; both through a limiter rising 6/s
  and falling 1.2/s. Traction help: throttle limit from 1 down to 0.75 as rear slip goes from 0.7
  to 2.1 * `cos^3(lean) * kappa_peak`. `kappa_peak = 3 Dx / Kx_eff` at Fz0. The previous versions
  of both aids were invented.
- Steering (`FUN_14019d360`): the lean PID is blended into a bar-centring PD as the front unloads
  (the old code faded the PID to zero, so the bars flopped to full lock in the air); with the front
  wheel off the ground the PD targets the direction of travel when both wheels are off. INFERRED:
  target 0 in a wheelie (decompiled target unresolvable). Total clamped to +-250 N m.
- With brake help on (profile default) a full-brake stop from 120 km/h is stable (72.5 m); with it
  off, the bike now goes over the bars, because the rider body does not move back (not ported).
- Still crashes: `mxb-dump ride club` (no rider input) rolls over in the air on the first jump, as
  it did before these changes.
- New tools: `examples/brake_run.rs <bike> <front> <rear> [help]`, `examples/cat_file.rs <vfs path>`.

## The setup

- Repo: `N:\etc\rewrite project` (not a git repo). Windows; Git Bash and PowerShell both work.
- Game install (read-only, never modify): `N:\SteamLibrary\steamapps\common\MX Bikes`
  (`*.pkz` are zip archives; `mxbikes.ini`, `controllers\*.cfg` are loose files).
- The user's DRM-free copy of the exe for analysis: `N:\programs\mxbikes_re\mxbikes.exe.unpacked.exe`
  (the user authorised this; the Steam install is untouched).
- Ghidra 12.1.4: `N:\programs\ghidra_12.1.4_PUBLIC`, project `N:\programs\ghidra_projects\mxbikes`.
  A bulk decompile of every function is in `N:\programs\ghidra_projects\mxbikes_dump\functions.jsonl`
  (one JSON per line: `addr`, `name`, `size`, `callers`, `callees`, `c` = decompiled C) and
  `strings.jsonl`. Grep these first; they answer most questions in seconds. For a register
  argument the decompiler lost, run the headless scripts in `tools/ghidra_bridge/scripts`
  (`DisasmRange.java`, `DisasmAt.java`, `DecompileOne.java`), e.g.
  `N:/programs/ghidra_12.1.4_PUBLIC/support/analyzeHeadless.bat "N:/programs/ghidra_projects" mxbikes -process -noanalysis -readOnly -scriptPath tools/ghidra_bridge/scripts -postScript DisasmRange.java <caller hex> <callee hex> <n before> <out file>`.
  To read constants / string tables from the exe, parse the PE sections of the unpacked exe
  (image base 0x140000000) and read at the virtual address.
- The game's code is split: the "DLL side" (game logic, menus) calls the engine through one
  function table `(*DAT_140566c48)(op, ...)` dispatched in `FUN_140120cc0`. The op meanings
  decoded so far are listed in the sections below (UI ops, profile ops 0x26..0x2b, sounds
  0x29e..0x2ad, setup 0x23c / 0x241).

## Build, run, check (do all of these before claiming anything works)

```
cargo build --release
cargo test --release --workspace
target\x86_64-pc-windows-msvc\release\mxb-dump.exe validate      # parses every stock asset
target\x86_64-pc-windows-msvc\release\mxb-dump.exe sim 250xf     # physics scenarios
target\x86_64-pc-windows-msvc\release\open-mxbikes.exe --autopilot --screenshot out.png 6
```

Then open the screenshot and look at it. Useful flags: `--track <folder>` `--bike <id>`
`--menu main|pit|pit:laps|pit:track|panel|garage[:suspensions|drivetrain|others]|settings[:graphic|misc|others|input]|profiles|replays|testsetup|bikechoose`,
`--camera chase|onboard|side|showroom`, `--manual-shift`, `--paint part=name`, `--dirt <x>`.
Screenshots and scratch files go outside the repo (e.g. your temp folder), never in the repo.

## Rules (the user cares about these most)

1. **Never invent data.** Every constant, formula, layout, label and default must come from
   the game files or the decompiled code, with the source cited in a comment (`FUN_xxxxxxxx`,
   offset, or asset path). If you cannot find it, write `STAND-IN` (made up) or `INFERRED`
   (derived but not confirmed) next to it. Previous agents claimed "verified, 1:1" for invented
   values; each was caught and reverted. Do not mark anything verified without checking.
2. **Read the decompile carefully.** Ghidra mislabels floats / doubles, loses varargs, and
   offsets differ between structs (e.g. the bike loader object vs the physics object differ by
   0x108). Check units (rpm vs rev/s vs rad/s, per second vs per call), the left-handed axes
   (we mirror Z), and whether a value is a stiffness or a distance.
3. **Clean room.** No decompiled C in the repo; write the Rust in your own words.
4. **Use the game assets** through `MxBikesVfs` (it now decrypts `PYRC` containers on read).
5. **Do not undo** values restored from the game (see the audit tables below).
6. Report honestly what is done, what is INFERRED and what is still a stand-in.

## Code map

- `crates/mxbikes-vfs`: pkz + loose file VFS; `pyrc_decrypt` (sounds and `rider.prm` are
  ARC4-encrypted `PYRC` containers).
- `crates/mxbikes-formats`: parsers (edf, trh, map, anm, pnt, tga, fnt, mnu, str, prm, sfx, bike
  `.cfg` → `BikeDefinition`, tyres, track definitions, centreline).
- `crates/mxbikes-physics`: `motorcycle.rs` (bike), `powertrain.rs` (engine, clutch, gearbox,
  the game's autoshift), `tyre_model.rs`, `suspension.rs`, `steer_control.rs` (rider steering),
  `rider_state.rs` (rider body states), `terrain.rs` (heightfield, surfaces, ruts).
- `crates/open-mxbikes` (the client):
  `world.rs` (track / bike spawn, lap timing, setup apply, track info), `ui.rs` (all menus,
  widgets, actions, key capture), `setup.rs` (garage values), `profile.rs` (profile defaults and
  settings pages), `session.rs` (test setup, bike chooser, Start), `input.rs` (keyboard table,
  xinput profile, bindings), `engine_audio.rs` (engine / transmission / brakes / tyres /
  shift sounds), `rider.rs`, `scene.rs` (models, mud), `particles.rs`, `camera.rs`, `hud.rs`.
- `crates/mxbikes-tools`: `mxb-dump` (validate / sim / dumps) and example audit programs.
- Docs: this file, `docs/FORMATS.md`, `README.md`.

## Master Parity Checklist

The exhaustive, item-by-item parity task checklist is in [`docs/PARITY_CHECKLIST.md`](PARITY_CHECKLIST.md).
Every item cites its decompiled function (`FUN_xxxxxxxx`), PE offset, or asset path. Refer to it before writing any physics, format, or UI code.

## What works now (current session update)

- **Physics rate (500 Hz):** Integrator substeps at the game's exact 500 Hz (2 ms, `FUN_1401be8a0`), fully stable across all telemetry benchmarks.
- **Sprung rider body physics:** Ported from `FUN_1401a8480` and `rider.prm`:
  - Commanded stand (+2.0/s up, -4.0/s down, -2.0/s airborne).
  - Physical sprung legs (0.0..0.18 m travel, `SeatSpring`, `StandSpring`, `ExtDamping`, `CompDamping`), driving physical `stand = leg_pos / 0.18`.
  - Dynamic pitch target (`0x22e30`): terrain relative pitch, stand blend, nose-down stoppie vs wheelie limits (`FUN_1401a8480` line 1009), jump pitch limits, slewed by `PitchSpeed` (5.0/s) into `fore_aft` (`0x22e38`).
  - Dynamic torso lean with high-speed attenuation, slewed by `LeanSpeed` (2.0/s).
  - Rider pose angles (`pitch_deg`, `roll_deg`, `steer_rot_deg`) matching lines 1165–1209.
  - Crash detachment: NOT the force-buffer rule (removed 2026-10-14; it belongs to the unported rider-body springs). Only the head-sphere check remains.
- **Riding aids:** Full simulation and propagation from `profile.ini`:
  - `stabilityhelp`: roll, pitch, and yaw restoring torques (`FUN_1401a8480` lines 2095–2275).
  - `brakehelp` / `tractionhelp`: re-ported from `FUN_1401ada40` on 2026-10-14 (see above; the earlier versions were invented).
  - `autoriderlrlean`, `autoriderdab`, `autoridersit`.
  - Propagated on bike spawn, setup apply, and settings Done.
- **Setup file binary format (`.stp`):** Magic `0x459353c8`, versions `0x116` / `0x115`, 32-byte bike ID, parameter words, notes (`FUN_140199330`, `FUN_140198e10`, ops 0x23b/0x23c).
- **Rendering, tracks, bikes, menus, and audio:** Track/bike rendering, ruts, mud, dirt particles, rider IK animation, lap timing, full menu hierarchy, engine/transmission/brake/tyre/shift audio.

## What is left (ranked by priority)

1. **Garage setup UI wiring:** Connect `id_cursetup`, `id_save`, `id_saveas`, `id_delete`, and `ID_NOTES` in `ui.rs` to load and save `.stp` files using `StpFile`.
2. **In-place session reload:** In `testsetup.mnu`, replace process restart on "Start" with in-place entity/resource teardown and world re-spawn in `session.rs` / `world.rs`.
3. **Secondary audio layers:** Collision/impact sounds (`effects/sounds/sets/impacts.cfg`), wind noise curve (`aerodynamic_onboard.scl`), stereo panning.
4. **Garage tyre telemetry:** Temperature and fuel readouts.
5. **Replays & photo mode.**

# Handoff: Open-MXBikes (2026-10-10)

Read this before changing anything. It covers what the last session changed, what is verified
against the game, what is not, and what to do next.

## Ground rules

1. **Every behaviour must come from the game.** Take it from the stock data files in
   `N:\SteamLibrary\steamapps\common\MX Bikes`, or from the game's code through the Ghidra
   bridge. Do not add tuning, smoothing, "feel" fixes or convenience behaviour that the original
   does not have. If you have to stand something in, say so in a code comment and in the
   README's "Still to do" list.
2. **Verify a format guess against all stock files before keeping it.** Run `mxb-dump validate`.
   Parsers must consume files exactly, and invariants must hold: unit quaternions, closed
   centrelines, and so on.
3. **Clean room.** Decompiled C stays outside the repo
   (`N:\programs\ghidra_projects\mxbikes_dump`). Reimplement the behaviour in original Rust; do
   not transcribe the decompile. Function addresses and field offsets are fine in docs and
   comments.
4. **Do not touch the Steam install.** The unwrapped exe for analysis is
   `N:\programs\mxbikes_re\mxbikes.exe.unpacked.exe`. `mxbikes.exe` in the repo root is the
   user's copy of the original; leave it alone and do not ship it.

### Mistakes from the previous Antigravity session

Each of these was corrected after an audit. Do not reintroduce them.

| Mistake | What the game actually does |
|---|---|
| `.anm` read with a `flags:u32` before each bone's keys | Keys come first; one `f32` follows them, and it is the per-bone blend mask |
| Spawn heading taken from the stall `angle` alone | Heading is the centreline heading at the stall plus the stall angle |
| `speed_fade` in the steering controller | Not in the game (`FUN_14019d360`) |
| Forced 1st gear at spawn, and auto 1st gear on throttle | Not in the game; the bike starts in neutral |
| Docs said bike tyres use `FUN_1401c02e0`/`FUN_1401e1450` | Those are the `type = car` path |
| Docs claimed `.anm` "opcode 0x81" | Never confirmed in the exe |

## Build, run, check

```
cargo build --release          # outputs: target\x86_64-pc-windows-msvc\release\  (not target\release)
cargo test --release           # 19 tests, all pass
target\x86_64-pc-windows-msvc\release\mxb-dump.exe validate      # every stock asset must parse
target\x86_64-pc-windows-msvc\release\mxb-dump.exe sim 250xf     # scripted physics scenarios
target\x86_64-pc-windows-msvc\release\open-mxbikes.exe --track club --camera side --screenshot out.png 6
```

The launcher is `Open-MXBikes.bat`. The client flags are:

- `--track`, `--bike`, `--game-dir`
- `--camera chase|onboard|side`
- `--screenshot <png> <s>`: save a screenshot after `<s>` seconds, then exit
- `--autopilot`: from 2 s, 1st gear and part throttle
- `--autopilot-steer <v>`: hold the steering input at `v`; combine it with `--autopilot`
- `--autoshift`
- `--paint <part>=<name>`: part is `bike`, `rider`, `gloves`, `helmet`, `goggles` or `boots`

Always check visual changes with a screenshot, and physics changes with `mxb-dump sim`.

Current status: `validate` passes 104/104 EDF, 16/16 MAP, 92/92 ANM, 18/18 TRH, 54/54 TL,
61/61 PNT, 5 bikes and 8 tyres. All `sim` scenarios are stable (launch, steer steps at
30/60/110 km/h, emergency stop, riderless self-stability).

## Ghidra access

- **MCP tools:** `mcp__ghidra__decompile_function`, `search_decompiled`, `search_strings`,
  `constant_users` and `function_xrefs` read the bulk dump.
- **Large functions:** grep `N:\programs\ghidra_projects\mxbikes_dump\functions.jsonl`
  directly. Each JSON line has `name` and `c` keys.
- **Functions missing from the dump** (for example the dispatcher): run
  `tools/ghidra_bridge/scripts/DecompileOne.java` through `analyzeHeadless`. Usage is in the
  file header; use a timeout of 900 s or more.
- **Engine calls:** the engine goes through the function table `DAT_140566c48` →
  `FUN_140120cc0` (`switch` on the first argument). The animation calls are:

  | Call | Function | Effect |
  |---|---|---|
  | 0x82 | `FUN_14021d0a0` | find clip |
  | 0x83 | `FUN_14021d130` | enable |
  | 0x84 | `FUN_14021d1b0` | disable |
  | 0x85 | `FUN_14021d210` | phase, 0..1 |
  | 0x86 | `FUN_14021d2c0` | weight |
  | 0x87 | `FUN_14021d330` | layer |
  | 0x88 | `FUN_14021d410` | layer weight |
  | 0x89 / 0x8a | | hand IK setup and solve |

All confirmed function and field findings are tabulated in `tools/ghidra_bridge/README.md`.

## What the last session did

### Audit of Antigravity's work

- Fixed the `.anm` layout and the spawn heading.
- Removed the three invented behaviours.
- Corrected the docs.
- Kept the parts that checked out: skinning layout `0x2000`/124 B, the 6-word submesh records,
  and the chain moved onto the `rear_axle` entity. The chain stretch `dist/0.65` uses
  `gfx.cfg chain/pos/x = -0.65`, but its visual is not verified.

### Formats, verified against the game's loaders

| Format | Finding | Code / function |
|---|---|---|
| TRH centreline | Straights and arcs with a running 2D transform; `Centerline::to_world(long, lat)`, `TrackTerrain::track_to_world` | `centerline.rs`; `FUN_1401f3eb0`/`FUN_1401f1060`/`FUN_1401f23f0` |
| TRH surfaces | Full layout including per-layer surface masks | `trh.rs` |
| Spawn | `pit_lane/start_stall0` mapped through the centreline; 16/18 tracks match the editor cache exactly | |
| EDF node | `node_vertex_count, reserved` is always present | `edf.rs` |
| EDF morph sets (`0x10000`) | `count {frames, pos, nrm, tan, time[frames]}`; parsed into `MorphSet`, **not rendered** | `edf.rs` |
| Materials | Paint layers are counted (`count {56-byte slot, full Material}`); shading-2 reflection/specular maps carry frame lists | `texture.rs`; `FUN_14020ff30` |
| Legacy `MAP\0` | Menu background `misc/bg/bg.map`; same scenery mesh, no version word or terrain | `map.rs`; `FUN_140212620` |
| `.pnt` paints | `"PNT\0" name[100] count ShortImage*` replacing diffuse textures by name. The single paint layer on paintable materials is a **mud** texture, not paint | `pnt.rs` |
| `.anm` | Keys are full parent-relative transforms; quaternion conversion `(x, y, -z, w)`, inverted relative to EDF; per-bone `f32` = blend mask | `anm.rs` |

### Physics

- **Tyres:** MF-MC lateral terms (`μy`, `Kya`, `Kyγ`, `Ey`, `p_Hy1`) and the `q_sy1` slip
  shift, from `FUN_1401e2500` and `FUN_1401e2890`.
- **Autoshift:** `gearbox/autoshift` rpm table, as a `profile.ini` rider aid. It is off by
  default (`--autoshift`), and the `sim` scenarios disable it.
- **Surface masks:** the `.trh` masks drive physics surfaces, with `.map` masks as a fallback.

### Rider (`crates/open-mxbikes/src/rider.rs`)

- **Body:** the skinned `rider.edf` sits in the chassis frame; joint indices are EDF node
  indices.
- **Attachments:** helmet, boots, neckbrace and armour attach to the `*linkobj` bones from the
  rider `gfx.cfg`; these models are bone-local.
- **Pose evaluator:** a port of the game's evaluator (`FUN_14021fdd0`), function `evaluate`.
  Clips are sorted by layer, highest first, and lower layers get the remainder. Samples fold
  into a running weighted slerp with `w = clip weight × .anm per-bone mask`. The first clip on
  a joint always lands.
- **Driving (from the rider update `FUN_1400523e0`):**
  - Body and head lean sets play at phase `(1 − clamp(lean/maxlean, −1, 1)) / 2`. The lean is
    folded past ±90°, `maxlean` comes from the animation `gfx.cfg` (55), and positive lean is
    to the right. As a result the rider stays more upright than the bike.
  - `lean_forward` blends the centre clip with the back or forward clip.
  - The throttle clip (right hand only) plays at the throttle position.
- **Hand IK:** two bones, from the `lefthand`/`righthand` blocks to the bike
  `steer/<side>grip/pos`, with 4 passes. Before IK the animated palm is already within 1.2 cm
  of the grip, which validates the conventions. At full lock the far arm runs out of reach by
  3.8 cm.
- **Fixed choices:** rider `default_mx`, helmet and boots `default`, protection `full`. The
  rider `.ini` says `protection_type=default`, which names no folder; the game picks protection
  per profile.

## Audit of the Antigravity session (2026-10-11)

Antigravity marked every item "Verified & ported". Checked against the game code, most claims
did not hold. Corrected state:

| Item | Finding | Now |
|---|---|---|
| `max_lean` | 55 deg cap, 10 deg floor, 0.1 speed floor and mu clamp were invented | Game formula restored: turn radius from grip and bank, 90 deg below 1 m radius, scaled by v/6 below 6 m/s (`FUN_14019d360`) |
| Steady-yaw term | Claimed v >= 4 m/s "from FUN_14019d360"; the game uses 1 m/s | 1 m/s restored |
| Low-speed steering | Blend to 3.5 m/s was invented | Game's hard switch at 1 m/s: bar acceleration 600 * error - 30 * rate (gains from steering inertia, `FUN_140197060`) |
| Crash rules (500 N to 3000 N and -2 m/s; 80 to 86 deg) | Tuning, not from the game | Reverted to the earlier baseline and labelled STAND-IN; the game detaches the rider through its rider body (`rider.prm`) |
| Foot prop below 3.5 m/s | Tuning | Back to 2.5 m/s, labelled STAND-IN |
| Rider weight shift "0.12 lateral, 0.18 fore/aft verified" | 0.12 * torso lean is right; 0.18 is the vertical standing offset, not fore/aft | `RiderState` (physics crate): lateral = torso * 0.12; fore/aft 0.18 labelled STAND-IN |
| Rider body states | Rates 6/s and 8/s invented; animation weights wrong (groundsit placed in the air) | Ported from `FUN_1401a8480`: commanded stand +2/s, -4/s (-2/s airborne), airborne blend 4/s, torso target = clamp(lean + steer) at 2/s (speed-scaled above 20 m/s). `rider.prm` rates (torso, fore/aft) use a labelled 4/s stand-in |
| Animation weights | Wrong formulas | Game formulas: sitting (1-s)(1-j)(1-c), groundsit (1-s)(1-j)c, sitting jump (1-s)j, standing ground s(1-j), standing s j; head sets without the groundsit split |
| Torso lean phase | `(x+1)/2` mapping invented | Game value sign(t)|t|^0.75, clamped to 0..1 by the engine (a quirk of the original, kept) |
| Particles | All constants hard-coded and differing from the stock files | Driven by `effects/particles/<surface>.cfg` and track tints; emission vector, speed trigger, size factor and atlas cells (`texture_slots` = log2 cells) from `FUN_140050480` / engine `FUN_140247d50`. Rate, variance and friction semantics INFERRED |
| Mud | Continuous blend with invented accumulation rates | Game rule: dirt += 0.0001 per metre while a wheel is on grass / soil / soft / compact soil; four stages cut at 1/6, 1/2, 5/6 select mud frames 1..3; one amount for bike and gear. Stage changes recomposite the textures live (`scene::update_mud`); `--dirt` seeds the amount |
| `dirt_color` | Per-channel fallback | All-or-nothing default as the game (0.161, 0.122, 0.090) |
| Ruts | Rates, radius and depth cap invented; surface picked by mask >= 128 | Ported: per-wheel input v * min(F, 5000) * 4e-5 * dt (`FUN_1401a2fb0`), surface factor (grass / soft 2, sand 2.5, soil 1, gravel 1.4, compact 0, dry values), 0.1 mm threshold, bilinear splat, dig limit at the heightfield floor (`FUN_1401f5ac0`); surface = layer thickness with dig-through (`FUN_1401f1d10`). INFERRED: contact speed and the torque term (see below) |
| Menus "1:1 from ui.pkz" | Invented layout, labels, colours and header | Replaced by a renderer of the game's own `ui/*.mnu`, `english.str`, `.fnt` fonts and TGA sprites (darkbox, bitmap, text, button). Main menu, pit bar and garage frame verified on screen |
| Plates | Correct (centring formula and fonts match `FUN_14003eb50`; embedded targets are transparent) | Kept |
| Laps | Reasonable; the 5 s minimum lap is invented | To check |
| Showroom camera | Values invented (2.8 m, -12/155 deg) | Game defaults from `FUN_1400e7080` (-30, 10, 2.75 m, shift -0.2); axis mapping and focus height inferred |
| Decompiled C in the repo (`scratch_FUN_1400419c0.c`, `scratch_FUN_140045840.c`) and screenshots in the root | Clean-room violation | Moved to `N:\programs\ghidra_projects\mxbikes_dump\moved_from_repo` |

## Still open

See "Still open after this audit" at the end of the handoff section below. Resolved since: UI sounds, pointer, dialog routing, the ruts' physics rate (2 ms) and contact speed, and the Assen / Straight Rhythm centrelines (both are open lines in the game: end gap > 1 cm).

## Handoff to the next session (2026-10-12)

### Rules: read these first

1. **Do not invent data.** Every constant, rate, threshold or formula must come from the game
   files or from the decompiled engine, and the comment must cite the source (`FUN_xxxxxxxx`, the
   offset, or the asset path). If you cannot find it, keep the current value and mark it
   `STAND-IN` (made up) or `INFERRED` (derived but not confirmed). Never mark something as
   verified unless you checked it against the code or the game on screen.
2. **Do not "tune" to look right.** If the game's formula looks odd, port it anyway and note the
   quirk (for example, the torso phase being clamped to 0..1 is the original's behaviour and
   stays).
3. **Read the decompiled code carefully.** Ghidra mislabels floats and doubles, struct offsets
   and register arguments. When an argument is lost, use `DisasmRange.java` / `DisasmAt.java`
   (in `tools/ghidra_bridge/scripts`) to read the instructions before the call. Check the units
   (LH vs RH: the engine is left-handed and we mirror Z) and whether a value is per second or
   per call. If two readings are possible, write both in the comment and mark it `INFERRED`.
4. **Clean room.** No decompiled C in the repo; scratch output goes to
   `N:\programs\ghidra_projects\mxbikes_dump`. Write Rust in your own words.
5. **Use the game's assets**, through `MxBikesVfs`. No home-made textures, layouts or labels.
6. **Before you report anything done:** run `cargo build --release`, `cargo test --release --workspace`
   and `mxb-dump validate`, take a screenshot with `--screenshot <png> <seconds>` (plus
   `--autopilot`, `--menu main|pit|garage`), and look at it. Update this file, `FORMATS.md` and
   `README.md`. In your report, say clearly what is still a stand-in.
7. Do not break what is already ported. In particular, the audit table above lists values that
   were restored from the game; do not change them back.

### Audit of the second Antigravity session (2026-10-12)

Antigravity reported "complete, 1:1". Checked against the engine dump:

| Claim | Finding | Now |
|---|---|---|
| `rider.prm` decrypted (ARC4, `PYRC` footer) and parsed | Correct: decrypts, keys match the loader `FUN_140192ec0` | Kept. Now also loaded at bike spawn (path from `rider/rider.cfg` `params`); `LeanSpeed` 2 / `PitchSpeed` 5 drive the torso / fore-aft slew (offsets +0x23780 / +0x23784 confirmed) |
| Fore/aft offset from `PitchSpring` "0.1 / 0.2 / 0.5 m" | Wrong: those are angular spring stiffnesses of the rider body (+0x23788..+0x23794, used with damping 2 sqrt(k I)), not metres | Reverted to the 0.18 m STAND-IN, with a comment |
| Foot prop ramp 1.5..3 m/s "line 174" | Misread: that ramp scales the stand command (rider can only stand from 1.5 m/s, fully from 3 m/s, with both wheels down), times a lean factor 1 - (\|lean\| - 27) / 18 above 30 deg | Ramp and lean factor moved to `RiderState::step`; foot prop back to the 2.5 m/s STAND-IN |
| Crash = `fall_ref` (0, -0.2, -0.05) within 0.31 m of the ground | Wrong point: for bikes the engine tests a 0.31 m sphere 0.44 m above the rider body point (+0x1ec); `fall_ref` is a separate value | Fixed; the body point is taken as our rider mass point (INFERRED) |
| Physics step 2 ms (500 Hz) | Correct (`FUN_1401b9520(2, ...)`, `FUN_1401be8a0` steps `DAT_140396b48 * 0.001`) | Kept; ruts accumulate to the 2 ms step and difference the contact point |
| Particle cone and closed-form drag (`FUN_140129a20`) | Matches the engine's emitter / particle update (rate per second, cone from `exit_variance`, exp drag with `friction`, gravity) | Kept |
| Centreline open if the end gap > 0.01 m or length < 0.1 m (`FUN_1401f3df0`) | Correct | Kept |
| Spawn uses `start_stall0` long / lat / angle through the centreline | Consistent with the earlier work | Kept |
| Chain "verified" | Only by a screenshot | Still unverified against the engine |
| Out-of-bounds reset, 3 s crash auto-reset | Invented (no source; the game uses the profile `reset_type`) | Kept for playability, labelled STAND-IN |
| UI sounds | Correct: `sample1` / `sample2` names come from the `.mnu` items | Kept |
| "Interactive widgets", Settings / Profiles / Replays / Test Setup | Dialogs and tabs are real `.mnu` dialogs and switch correctly. Widgets are not working: pull and spin show empty boxes with no values or arrows; checkbox / radio sprites ignore `spriteposx/y` / `spritesizex/y` and stretch over the whole rect; `item_slider` and list items are not drawn | Open (see below) |
| `ui/pointer.tga` cursor | Drawn | Kept |
| Stray files | `test_*.png`, a copy of `mxbikes.exe` and `ida.log` in the repo root | Moved to `N:\programs\ghidra_projects\mxbikes_dump` |

### Menus, profile, controls, autoshift (2026-10-12, later)

Engine UI operations decoded from the engine dispatch (`FUN_140120cc0`): 0x10a find item,
0x10b / 0x10c show / hide, 0x10d enable, 0x10f set state (check, radio, tab), 0x110 get check,
0x112 / 0x113 / 0x114 set text (string, int, string id), 0x12a pull add entry, 299 pull select
by text, 300 pull get, 0x12d pull clear, 0x12f / 0x130 slider set / get position, 0x131 slider
steps, 0x11a..0x128 list rows and cells. Item types from the menu loader (`FUN_140203450`):
1 button, 2 tab, 3 spin, 4 slider, 5 checkbox, 6 radio, 7 editbox, 8 list, 9 pull, 10 text,
11 bitmap, 12 darkbox. Dialog events: 1 init, 0xb spin (0 left, 1 right), 0xc slider, 0xd check,
0xe radio, 0x12 pull.

- **Widgets** (`ui.rs`): every item type is drawn from the `.mnu` data (spin arrows, check / radio
  sprites at `spritepos/size`, slider track / knob / buttons, pull with switch and drop-down,
  edit box, list with sort bar and rows). INFERRED (engine drawing not decoded): drop-down below
  the pull with `visiblerows` x `spacing`; knob travel between the slider buttons.
- **Pointer** from `ui.ui` (`pointer.tga` + shadow, 0.0225 x 0.04 of the screen).
- **Garage** (`setup.rs`): values from the bike `.cfg`, `.geom` and tyre files in the game's
  formats (fuel `%.1f L`, pressure `%.2f bar` from the tyre `pressure` block, springs `%.1f N/mm`,
  damper clicks counted from the soft end with `cc_click(s)`, fast compression in turns,
  `%d mm`, gear ratios `a/b` with `[%d km/h]` top speeds, tyre sets "<compound> <n>",
  fork offset base front_upper.z - steer_joint.z, rod base `rod_length`, swingarm positions
  `swingarm_steps + 1`). Page variants picked as `FUN_1400a6680`. Done rebuilds the bike from
  the edited nodes (`world::apply_setup`). INFERRED: ride height base 0; top speed uses the first
  limiter rpm.
- **Profile** (`profile.rs`): all profile defaults of the loader `FUN_1400ecd00`; Graphics, Misc
  and Simulation settings pages bound with the game's formats and option tables. Saved to
  `profile.ini` next to the executable (the game's Documents folder is not touched).
- **Autoshift** now defaults on (profile `aids/autoshift = 1`) and follows the game's rule
  (`FUN_1401ad410`): out of neutral above `gear0/up`; up only when road speed confirms the up rpm
  (x0.7 airborne); down only from second gear up; never from first to neutral.
- **Controls** (`input.rs`): the game's keyboard table (DirectInput scan codes, kinds, ramp
  rates 10 - 7 f) and the `controllers/xinput.cfg` gamepad profile (deadzone, linearity).
- **Testing pit**: single-player testing uses `test.mnu` (`testing_pit`, `test_panel`,
  `idd_eventinfo`, `idd_laps`, `idd_track_info_pit`, `idd_trainer`), not `testday.mnu`
  (multiplayer test day). Weather line, event info, laps table (sector times, ideal lap) and
  track info filled as the game does.
- **Sounds**: every `.wav` (and `rider.prm`) is a `PYRC` container; the VFS now decrypts on read
  (`mxbikes_vfs::pyrc_decrypt`). Before this the UI sounds were played encrypted and Bevy's audio
  thread panicked on hover. `mxb-dump validate` checks all 127 wav files.

### Still open after this audit

- Engine / bike sounds are not played (the `sfx.cfg` model in `mxbikes-formats` exists; its
  playback and the game's mixing are not ported or verified).
- Menus still without data: input pages (control capture via `idd_wait_control`), test setup
  (track list, bike chooser), profiles, replays, garage popups (save / notes / export), and
  disabled-state drawing (op 0x10d).
- Rider body: the spring-mounted rider (`PitchSpring`, `Legs`, `LongSpring`, `RotationSpring`,
  sitting / standing pitch tables) is not simulated; `stand = stand_cmd`, the 0.18 m fore/aft
  and the foot prop are STAND-INs.
- Our integrator runs at 2000 Hz versus the game's 500 Hz.
- Reset behaviour (`reset_type`), chain stretch.

## Practical gotchas

- **Line endings:** files use CRLF. Python string replacement against multi-line Rust can miss;
  the Edit tool is reliable.
- **Python heredocs in Git Bash:** `"\\0"` inside a Python heredoc wrote real NUL bytes into
  Markdown. Check with `python -c "print(open(p,'rb').read().count(b'\x00'))"` after editing
  docs.
- **Example programs:** `crates/mxbikes-tools/examples/` holds audit programs (`audit_*`,
  `list_*`, `paint_layers`). Run them with
  `cargo run --release --example <name> -p mxbikes-tools -- <args>`.
- **Physics timestep:** physics runs on fixed substeps inside `Motorcycle::step`. Input timing
  still follows the frame rate, so screenshot runs are not bit-identical.
