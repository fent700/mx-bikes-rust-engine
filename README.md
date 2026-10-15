# Open-MXBikes

An open-source clean-room reimplementation of MX Bikes written in Rust on the Bevy 0.15 engine.

Like OpenMW or OpenRA, this project is purely an engine rewrite. It contains no proprietary models, textures, tracks, or sound assets. At runtime, it reads directly from the `.pkz` archives of a legally purchased copy of MX Bikes (auto-detected from your Steam install, or specified via `--game-dir`).

---

## What's Implemented

- **500 Hz Physics Pipeline**: Multibody motorcycle chassis, 3-stage front fork damping, rear rising-rate linkage kinematics (~330 mm wheel travel), and steer inertia.
- **Pacejka MF-MC Tyre Model**: Pacejka 2002 motorcycle formulation with combined slip, camber thrust ($q_{dz8}$), dynamic contact patch relaxation, and per-surface friction / soil deformation.
- **Powertrain & Gearbox**: Crank inertia, torque/drag curves from `.engn`, multi-plate clutch with coupled drivetrain inertia solving, autoclutch, and shift assistance (throttle lift and blip).
- **Format Parsers (`mxbikes-formats`)**:
  - 3D Models: `.edf` geometry, bone hierarchies, materials, LOD switching, and morph targets.
  - Animations: `.anm` bone track evaluator with inverse quaternion frames, bone masking, and weighted slerp blending.
  - Tracks: `.trh` heightmaps, physics surface layer masks, `.map` trackside scenery, `.amb` sky/lighting, and `.tl` timing lines.
  - Setups: Binary `.stp` read/write with clicker resolution for forks, shocks, gearing, and tyre pressures.
- **Audio (`engine_audio`)**: Speed-scheduled engine on/off throttle sample blending, limiter cut, transmission whine, road tyre scrub, and aero wind noise.
- **UI & Controls (`open-mxbikes`)**:
  - Menu system rendering stock `.mnu` dialogs with full button dispatch, edit boxes, sliders, and drop-downs.
  - Profile system reading and writing `profile.ini` for graphics, audio, views, input settings, and riding aids.
  - Native XInput gamepad and keyboard support with configurable deadzones, linearity, and smoothing.

---

## Workspace Structure

```
crates/
  mxbikes-vfs/        Transparent .pkz archive mounting with loose-file mod overrides
  mxbikes-formats/    Parsers for PiBoSo binary and text formats (EDF, MAP, TRH, ANM, STP, etc.)
  mxbikes-physics/    Simulation core: motorcycle kinematics, Pacejka MF-MC tyres, powertrain
  mxbikes-tools/      CLI tool (`mxb-dump`) for validating assets, simulating runs, and extracting data
  open-mxbikes/       Bevy game client, UI renderer, and camera system
```

---

## Getting Started

### Prerequisites

- A legal installation of MX Bikes on PC (Steam or PiBoSo standalone).
- Rust 1.80+ (`rustup default stable`).

### Building and Running

Auto-detect your game directory and boot straight into the menu:

```bash
cargo run --release -p open-mxbikes
```

Or launch directly onto a specific track and bike:

```bash
cargo run --release -p open-mxbikes -- --track club --bike 250xf
```

Windows users can also double-click `Open-MXBikes.bat` in the root folder.

### Common Launch Flags

- `--track <name>`: Track to load (`club`, `practice`, `maryland`, etc.). Defaults to menu background.
- `--bike <id>`: Bike to ride (`250xf`, `250x`, `350xf`, `450xf`, `450sm`).
- `--game-dir <path>`: Explicit path to MX Bikes installation if not auto-detected.
- `--camera <chase|onboard|side>`: Starting camera mode (can also be toggled in-game).
- `--paint <part>=<name>`: Apply livery paints (e.g. `--paint bike=blue --paint rider=orange_blue`).
- `--dirt <0.0 - 1.0>`: Set initial bike and rider mud accumulation.

---

## Controls

The default keybindings match the stock game's layout:

| Action | Keyboard | Gamepad (XInput) |
|---|---|---|
| Steer / Lean | A / D or ← / → | Left Stick X |
| Throttle | W or ↑ | Right Trigger |
| Front Brake | S or ↓ | Left Trigger |
| Rear Brake | Left Alt | Right Stick Down |
| Clutch | C | Left Stick Down |
| Shift Up / Down | Q / Z | Right Bumper / Left Bumper |
| Rider Lean Fore / Aft | W / S | Right Stick Y |
| Sit / Stand | E | B Button |
| Starter | K | Start |
| Reset to Track | R | Right Stick Click |
| Change Camera | V | Y Button |
| Esc / Pit Menu | Esc | Back / View |

---

## Credits

- **PiBoSo**: Creators of MX Bikes and its physics data formats.
- **Claude Opus 5.5**: Architectural planning, reverse engineering and decompilation analysis, math derivation for tyre and linkage mechanics, and Rust systems design.
- **Bevy Engine**: The modular Rust game engine powering rendering and entity dispatch.
