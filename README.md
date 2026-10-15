# Open-MXBikes

A clean-room reimplementation of **MX Bikes** in **Rust** on the **Bevy** engine (0.15). It runs
on the original assets from a legally purchased copy of the game.

It follows open engine recreations such as OpenMW, OpenRA and OpenRCT2: the repository contains
engine code only, and every model, texture, track and physics parameter is read from your install
at runtime.

---

## Legal & clean-room approach

1. **No proprietary assets included.** No models, textures, sounds, tracks or physics files are
   distributed.
2. **Purchased game required.** The VFS mounts the original `.pkz` archives from your Steam
   install (auto-detected, or `--game-dir`).
3. **Formats were decoded from the data files** and validated against every stock asset
   ([`docs/FORMATS.md`](docs/FORMATS.md)). Behaviour that the data alone cannot pin down (rider
   steering controller, surface ids) was studied in a locally unwrapped copy of the user's own
   executable through the Ghidra bridge, and reimplemented in original Rust; no game code or
   decompiled output is included in this repository.

---

## Workspace

```
crates/
  mxbikes-vfs/        .pkz (ZIP) mounting, case-insensitive paths, loose-file mod overrides
  mxbikes-formats/    PiBoSo formats: text configs, EDF, MAP, TRH, TL, embedded textures, bike/tyre/track definitions
  mxbikes-physics/    multibody bike, MF-MC tyres, powertrain, fork + rising-rate linkage, terrain ground
  mxbikes-tools/      mxb-dump: validate / export / simulate against the installed game
  open-mxbikes/       Bevy client
tools/
  ghidra_bridge/      MCP server serving Ghidra analyses to Claude Code (+ DumpProgram.java exporter)
  ida_bridge/         IDA Pro MCP notes (IDA currently fails its licence check)
  inspect_game.py     install / toolchain diagnostic
docs/FORMATS.md       format specifications
```

## Status

What comes straight from the game data:

| Area | Source |
|---|---|
| Bike meshes, part hierarchy, LOD groups | `.edf` via `gfx.cfg` → `.hrc` |
| Bike textures (diffuse, normal, embedded) | `.edf` material tables |
| Wheels / tyres (visual) | `tyres/<set>/*.edf` |
| Terrain geometry | `.trh` |
| Terrain texturing (4 layers, masks, tiling) | `.map` terrain block |
| Trackside scenery (trees, fences, tents, vehicles, banners) | `.map` scenery + materials |
| Sky, background, sun, ambient, fog | `.amb`, sky `.edf` |
| Spawn | `.rdf` `pit_lane` start |
| Engine torque / friction curves, idle, limiter, inertia | `.cfg` + `.engn` |
| Clutch capacity / dead zone, gearbox ratios, shift times, sprockets, efficiency | `.cfg` |
| Brakes (lever force, leverage, master cylinder, pad torque) | `.cfg` |
| Chassis mass / inertia, aero CdA / lift | `.cfg` |
| Fork spring, preload, air spring, oil level, 3-stage damping | `.cfg` setup values |
| Shock spring, preload, bump/fast-bump/rebound, bump stop | `.cfg` setup values |
| Rear linkage kinematics (rising rate, about 330 mm wheel travel) | `.geom` hard points |
| Steering geometry, fork offset, lock, steering damper | `.geom` + `.cfg` |
| Tyre forces (Pacejka MF-MC with camber thrust, combined slip) | `.tyre` coefficients |
| Rider steering: lean-target PID with speed-scheduled `spg/sdg/sig`, `KYaw` yaw term, grip-limited and rate-limited target lean, front-load fade, ±250 N·m | `.cfg` `steer` block, control law from the game |
| Surface ids per terrain layer | game's surface enumeration |
| Tyre vertical stiffness / damping, radii, inertia | `.tyre` |
| Surface grip / rolling resistance per terrain layer | `.tyre` compounds + `.trh` surfaces |

Results checked with `mxb-dump sim`: static sag of 30% at the rear, a riderless bike that
self-stabilises at 30 and 50 km/h and capsizes at walking pace, a 250F top speed of about
123 km/h, and full-throttle 1st-gear wheelies.

What was reverse-engineered and implemented:

* **Rider**: the skinned `rider.edf` body is rendered on the bike and posed with a port of the game's pose evaluator, which uses layers, the per-bone masks from `.anm` and weighted slerp. The body and head lean sets play at the game's lean phase for the bike angle, so the rider stays more upright than the bike in turns. The back/forward variants blend in with fore/aft weight shift, and the throttle clip moves the right hand. Helmet, boots, neckbrace and armour attach to the link bones named in the rider `gfx.cfg`. Hands follow the bars by two-bone IK from the rider `lefthand`/`righthand` blocks (wrist palm offset, elbow, shoulder) to the bike's `steer` grip positions. The animated pose already puts the palm within 1.2 cm of the grip before IK. `.anm` keys are full parent-relative transforms with inverted quaternions; this was checked against the EDF rest pose.
* **Assets**: every stock file parses (`mxb-dump validate`: 104 EDF, 16 MAP, 92 ANM, 18 TRH, 54 TL). This needed the game's EDF morph-set layout (cloth flags), counted paint layers, shading-2 map frames and the legacy `MAP\0` menu background.
* **Tyre evaluator**: the lateral MF-MC terms (`μy`, `Kya`, `Kyγ`, `Ey`, `p_Hy1`) and the `q_sy1` rolling slip shift follow the game's bike tyre evaluator, with camber thrust and lateral relaxation.
* **Track coordinates**: the TRH centreline is decoded, so `.rdf` start stalls, gates and `.tl` lines map to the world as the game does. The bike spawns in `pit_lane/start_stall0`.
* **Surfaces**: per-layer physics masks come from the `.trh`, with the `.map` blend masks as fallback.
* **Paints**: `.pnt` liveries for bike, suit, gloves, helmet, goggles and boots replace the matching diffuse textures. Pick them with `--paint <part>=<name>`, e.g. `--paint bike=blue --paint rider=orange_blue`. Without one, the embedded textures are used.
* **Gearbox**: the `autoshift` rider aid uses each bike's `gearbox/autoshift` rpm table. It follows the profile aid, on by default as in the game (`aids/autoshift = 1`, `FUN_1400ecd00`); `--manual-shift` turns it off.
* **Launcher**: `Open-MXBikes.bat` ready for one-click release launching with Bevy 0.15.

Still to do:

* **Rider animation**: standing, airborne and ground-sit variants, torso lean, feet-down and stunt clips. These need the rider body simulation, whose parameters appear to be in the binary `rider.prm`. The stock animation set has no brake, clutch or gear-change clips, so those layers do nothing in the game either.
* Menu widgets and dialogs past the main menu, pit bar and garage frame (see `docs/HANDOFF.md`, "Still open").


---

## Quick start

```powershell
python tools/inspect_game.py                          # verify install + toolchain
cargo test --workspace                                # unit tests (use real stock data values)
cargo run --release -p mxbikes-tools -- validate      # parse every stock asset
cargo run --release -p mxbikes-tools -- sim 250xf     # physics scenarios on flat ground
cargo run --release -p mxbikes-tools -- ride club     # straight-line run on a real track
cargo run --release -p mxbikes-tools -- edf bikes/250xf/250xf.edf out/   # export textures + obj
cargo run --release -p mxbikes-tools -- track club out/                  # terrain layers, masks, heightmap
cargo run --release -p open-mxbikes -- --track club --bike 250xf
```

Client flags: `--track <name>` (club, practice, maryland, ...), `--bike <id>` (250xf, 250x,
350xf, 450xf, 450sm), `--game-dir <path>`, `--camera chase|onboard|side`,
`--screenshot <file.png> <seconds>`, `--autopilot`, `--autopilot-steer <v>` (hold steer input `v`; with `--autopilot` it steers while riding), `--autoshift`, `--paint <part>=<name>` (part: bike, rider, gloves, helmet, goggles, boots), `--dirt <amount>` (mud stages at 0.1666 / 0.5 / 0.8333), `--menu main|pit|garage[:suspensions|drivetrain|others]|settings[:graphic|misc|others|input]|profiles|replays|testsetup`, `--manual-shift` (the profile's autoshift aid is on by default, as in the game).

### Controls

The game's defaults: the keyboard table the game registers and its `controllers/xinput.cfg`
gamepad profile. Keyboard axes ramp at the game's rates.

| Action | Keyboard | Gamepad (XInput) |
|---|---|---|
| Steer | ← / → | left stick X |
| Throttle | ↑ | right trigger |
| Front brake | ↓ | left trigger |
| Rear brake | Left Alt | right stick down |
| Clutch | C | left stick down |
| Shift up / down | Q / Z | right / left bumper |
| Rider lean forward / back | W / S | |
| Rider lean left / right | A / D | |
| Sit / stand | E | B |
| Starter | K | |
| Reset | R | right stick button |
| Change view | V | |
| Pit menu | Esc | Back |

---|---|---|
| Steer / lean | A / D, ← / → | left stick X |
| Throttle | W / ↑ | right trigger |
| Front brake | S / Space | left trigger |
| Rear brake | X / ↓ | left bumper |
| Clutch | Left Shift | right bumper |
| Shift up / down | E / Q | B / X |
| Rider weight forward / back | T / G | right stick Y |
| Starter (after stall) | Enter | Start |
| Reset to pits | R | Select |
| Camera (chase / onboard / side) | C | Y |

---

## Reverse-engineering tooling

* **Ghidra MCP bridge** (`tools/ghidra_bridge`), registered in `.mcp.json` as `ghidra`. Ghidra
  12.1.4 is installed at `N:\programs\ghidra_12.1.4_PUBLIC`. Its `check_binary` tool flags
  that the stock exe is SteamStub-encrypted.
* **IDA Pro MCP bridge**, registered as `ida-pro`. It needs a licensed IDA running with the MCP
  plugin on port 13337.
