# Open-MXBikes: 1:1 Parity Master Task Checklist

This document is the authoritative tracking checklist for achieving 100% clean-room 1:1 parity with the original MX Bikes engine (`mxbikes.exe.unpacked.exe`).
Every system, formula, constant, and layout must cite its decompiled function (`FUN_xxxxxxxx`), binary offset, or stock asset path.
**Zero invented constants. Zero tuning by feel. Clean-room verification.**

---

## Master Status Summary

| Area | Status | Verification Reference |
|---|---|---|
| **Physics Rate (500 Hz)** | [x] Complete | `FUN_1401be8a0`, `mxb-dump sim 250xf` |
| **Sprung Rider Body Physics** | [x] Complete | `FUN_1401a8480`, `rider.prm`, `rider_state.rs` |
| **Crash Detachment Buffer** | [x] Complete | `FUN_1401a8480` lines 1489–1499, unit tests |
| **Riding Aids (Brake, Traction, Stability)** | [x] Complete | `FUN_14006d050`, `FUN_1401a8480`, `motorcycle.rs` |
| **Setup File Binary Format (.stp)** | [x] Complete | `FUN_140199330`, `FUN_140198e10`, `setup_file.rs` |
| **Garage UI Setup Save/Load Wiring** | [x] Complete | `ui.rs`, `setup.rs`, `idd_garage` |
| **In-Place Session Reload** | [x] Complete | `session.rs`, `world.rs`, `ui.rs` |
| **Secondary Audio (Impacts, Aero, Panning)**| [x] Complete | `engine_audio.rs`, `impacts.cfg`, `aerodynamic_onboard.scl` |
| **Graphics Settings & Window Reconfig** | [x] Complete | `FUN_1400d4580`, `profile.rs`, `ui.rs`, `ApplyGraphicsSettings` |
| **All Menu Buttons & Dialog Action Dispatch** | [x] Complete | `FUN_1400d7ff0`, `FUN_1400e2640`, `FUN_1400a6680`, `FUN_1400b3dd0` |
| **Stock Asset Parsers (691 assets)** | [x] Complete | `mxb-dump validate` (0 failed across all tracks) |

---

## 1. Sprung Rider Body Physics (`FUN_1401a8480` & `rider.prm`)

- [x] **1.1. Dynamic Torso Pitch Kinematics (`0x22e30` in `FUN_1401a8480`)**
  - **Source:** `FUN_1401a8480` lines 970–1105; parameters loaded via `FUN_140192ec0` from `rider.prm`.
  - **Terrain Relative Pitch:** `rel_pitch = bike_pitch - terrain_pitch`.
  - **Stand Blending:**
    - If `rel_pitch >= 0`: `max_angle = standing_pitch_max_angle_forward * stand + sitting_pitch_max_angle_forward * (1 - stand)`.
    - If `rel_pitch < 0`: `max_angle = standing_pitch_max_angle_backward * stand + sitting_pitch_max_angle_backward * (1 - stand)`.
    - Rate blend: `rate = standing_pitch_rate * stand + sitting_pitch_rate * (1 - stand)`.
  - **Base Target:** `target = (accel - terrain_accel) / rate + rel_pitch / max_angle`.
  - **Stoppie vs Wheelie Sign Convention (`FUN_1401a8480` line 1009):**
    - Pitch <= 0.0 (stoppie / nose-down): `accel / stoppie_pitch_rate + pitch * fVar37 + target`.
    - Pitch > 0.0 (wheelie / nose-up): `accel / wheelie_pitch_rate + pitch / wheelie_pitch_max_angle + target`.
  - **High-Speed Roll Factor:** Forward speed > 1.5 m/s adds `0.25 * roll_accel`.
  - **Airborne Pitch:** Target evaluated from `-bike_pitch / jump_pitch_max_angle + forward_accel / jump_pitch_rate`.
  - **Manual Modulation:** Modulated by input `lean_forward` (`0x300`) and clamped to `[-1.0, 1.0]`.
  - **Slew Rate Filter:** Slewed into `fore_aft` (`0x22e38`) via `PitchSpeed = 5.0/s` (`0x23784`).
  - **File:** [`crates/mxbikes-physics/src/rider_state.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-physics/src/rider_state.rs).

- [x] **1.2. Rider Body Pose Angles (`FUN_1401a8480` lines 1165–1209)**
  - **Source:** `FUN_1401a8480` lines 1165–1209.
  - **Pitch Angle (deg):** Table blend from `sitting_pitch_backward/forward` and `standing_pitch_backward/forward`.
  - **Torso Lean (deg):** `(roll_deg / 90.0) * sitting_lean * (1 - stand) - (lean * torso_lean)`.
  - **Steer Rotation (deg):** `steer_angle * -0.5`.
  - **File:** [`crates/mxbikes-physics/src/rider_state.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-physics/src/rider_state.rs).

- [x] **1.3. Sprung Rider Legs & Physical CG Dynamics (`FUN_1401a8480` lines 1350–1585)**
  - **Source:** `FUN_1401a8480` lines 1350–1585; `rider.prm` blocks `legs`, `longspring`, `pitchspring`.
  - **Physical Leg Travel:** 0.0..0.18 m extension. Body `stand = leg_pos / 0.18`.
  - **Leg Spring & Damping:**
    - `k_legs = (Legs/StandSpring - Legs/SeatSpring) * stand + Legs/SeatSpring`.
    - Damping: `2.0 * sqrt(k_legs * rider_mass) * (if v_rel >= 0 { Legs/ExtDamping } else { Legs/CompDamping })`.
    - Vertical clamp: `[0.0, 2943.0]` N.
  - **Longitudinal Spring & Damping:** `k_long = 9000.0` N/m, `damping = 2.0 * sqrt(k_long * rider_mass) * 0.6`.
  - **Rotational Spring & Damping:** Interpolated sitting/standing by `stand`, multiplied by `(1.0 + abs(pitch) * SpringsFactor) * 0.5`.
  - **Chassis Feedback:** Reaction forces and moments applied to bike frame at rider mount point (`+0x3a0`).
  - **File:** [`crates/mxbikes-physics/src/rider_state.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-physics/src/rider_state.rs), [`crates/mxbikes-physics/src/motorcycle.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-physics/src/motorcycle.rs).

- [x] **1.4. Rider Detachment & Crash Invariant (`FUN_1401a8480` lines 1489–1499 & `FUN_1401a79f0`)**
  - **Source:** `FUN_1401a8480` lines 1489–1499, `FUN_1401a79f0`.
  - **Exact Detachment Rule:**
    - 25-frame rolling force buffer.
    - Lateral force limit: `|Fx| > 4000.0` N.
    - Longitudinal force limit: `|Fz| > 10000.0` N.
    - Vertical compression limit: `Fy > 15000.0` N.
    - Moving average vertical extension limit: `avg_Fy_ext < -2000.0` N.
  - **Trigger:** Sets `detached = 1`, crash state 10 (left fall) or 11 (right fall).
  - **File:** [`crates/mxbikes-physics/src/rider_state.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-physics/src/rider_state.rs).

---

## 2. Riding Aids Simulation & Profile Propagation

- [x] **2.1. Stability Help Restoring Torques (`stabilityhelp`, `stabilityhelp_scale`)**
  - **Source:** `FUN_1401a8480` lines 2095–2275; `profile.ini` aids block.
  - **Roll Restoring Torque:** `(roll_rate * stability_roll_rate + roll_angle * stability_roll_factor) * stabilityhelp_scale`.
  - **Pitch Restoring Torque:** `(pitch_rate * stability_pitch_rate + pitch_angle * stability_pitch_factor) * stabilityhelp_scale`.
  - **Yaw Restoring Torque:** `(yaw_rate * stability_yaw_rate + yaw_angle * stability_yaw_factor) * stabilityhelp_scale`.
  - **File:** [`crates/mxbikes-physics/src/rider_state.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-physics/src/rider_state.rs).

- [x] **2.2. Brake Help (`brakehelp`, op 0x266 / `DAT_140e557fc`)**
  - **Source:** `FUN_14006d050`, op 0x266.
  - **Modulation:** Modulates front brake pressure when front slip ratio exceeds lock threshold (-0.15) or chassis pitch rate indicates impending stoppie over-rotation.
  - **File:** [`crates/mxbikes-physics/src/motorcycle.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-physics/src/motorcycle.rs).

- [x] **2.3. Traction Help (`tractionhelp`, op 0x267 / `DAT_140e55800`)**
  - **Source:** `FUN_14006d050`, op 0x267.
  - **Modulation:** Attenuates engine throttle output when rear wheel longitudinal slip ratio exceeds peak grip slip (`> 0.15`).
  - **File:** [`crates/mxbikes-physics/src/motorcycle.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-physics/src/motorcycle.rs).

- [x] **2.4. Low-Speed Foot Dab & Sit Aids (`autoriderdab`, `autoridersit`)**
  - **Source:** `autoriderdab` (`DAT_140e5580c`), `autoridersit` (`DAT_140e55810`).
  - Foot prop triggered when speed < 2.5 m/s and lean angle > 5 deg.
  - Automatic transition to sit on jump landing and corner entry braking.
  - **File:** [`crates/mxbikes-physics/src/rider_state.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-physics/src/rider_state.rs).

- [x] **2.5. Profile Propagation to Motorcycle Entity**
  - Propagate all profile aids (`brakehelp`, `tractionhelp`, `stabilityhelp`, `stabilityhelp_scale`, `autoriderlrlean`) on spawn, setup apply, and settings Done.
  - **Files:** [`crates/open-mxbikes/src/main.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/main.rs), [`crates/open-mxbikes/src/world.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/world.rs), [`crates/open-mxbikes/src/ui.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/ui.rs).

---

## 3. Garage Setup File (.stp) Save & Load

- [x] **3.1. Binary `.stp` Parser & Serializer**
  - **Source:** `FUN_140199330` (op 0x23b, saver), `FUN_140198e10` (op 0x23c, loader), `FUN_140199e00` (validator).
  - Magic `0x459353c8`, version `0x116` / `0x115`, 32-byte bike ID, 2-byte subversion, 54 parameter words, variable-length notes string.
  - Unit tests verifying magic rejection, roundtrip serialization, and truncated inputs.
  - **File:** [`crates/mxbikes-formats/src/setup_file.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-formats/src/setup_file.rs).

- [x] **3.2. BikeSetup Export & Import Conversion**
  - Map `BikeSetup` config nodes, tyre compounds, tyre pressures, suspension clicks, and gearing to/from `StpFile`.
  - **File:** [`crates/open-mxbikes/src/setup.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/setup.rs).

- [x] **3.3. Garage UI Setup Buttons & Popups**
  - Directory enumeration for `id_cursetup` pull menu (`setups/<bike>/`).
  - Wire `id_save` to overwrite current `.stp` file (`FUN_14009bf20`) or prompt `idd_setup_save` if on `-Default-`.
  - Wire `id_saveas` to open `idd_setup_save` dialog and write new `.stp` file.
  - Wire `id_delete` to prompt `idd_setup_delete` and delete selected `.stp` file on `id_yes`.
  - Wire `ID_NOTES` to display and edit setup notes in `idd_setup_notes`.
  - **File:** [`crates/open-mxbikes/src/ui.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/ui.rs).

---

## 4. In-Place Session Reload

- [x] **4.1. Replace Process Relaunch with Clean In-Place Teardown**
  - When clicking "Start" (`id_start`) in `testsetup.mnu`:
    - Despawn track meshes, scenery, collision map, atmosphere, and audio via `WorldEntity` query.
    - Despawn bike entity, parts hierarchy, and rider model cleanly.
    - Load new track and bike definitions from `Vfs` in-place.
    - Re-initialize world, spawn point, lap timing, and cameras without process exit.
  - **Files:** [`crates/open-mxbikes/src/session.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/session.rs), [`crates/open-mxbikes/src/world.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/world.rs), [`crates/open-mxbikes/src/ui.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/ui.rs), [`crates/open-mxbikes/src/main.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/main.rs).

---

## 5. Audio Pipeline Expansion

- [x] **5.1. Impact & Scrape Audio**
  - Load and parse `effects/sounds/sets/impacts.cfg` across all 8 material sets.
  - Trigger collision and chassis scrape sounds based on frame contact forces (`BodyContact.impact_impulse`, `scrape_speed`).
  - **Files:** [`crates/mxbikes-formats/src/sfx.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-formats/src/sfx.rs), [`crates/mxbikes-physics/src/motorcycle.rs`](file:///n:/etc/rewrite%20project/crates/mxbikes-physics/src/motorcycle.rs), [`crates/open-mxbikes/src/engine_audio.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/engine_audio.rs).

- [x] **5.2. Aerodynamic Wind Noise**
  - Load wind attenuation curve from `aerodynamic_onboard.scl`.
  - Modulate volume and pitch based on bike forward airspeed (`speed_kmh / 200.0`).
  - **File:** [`crates/open-mxbikes/src/engine_audio.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/engine_audio.rs).

- [x] **5.3. Spatial Stereo Azimuth Panning**
  - Calculate listener-relative azimuth angle via `listener_right` dot product and apply distance/stereo attenuation curves.
  - **File:** [`crates/open-mxbikes/src/engine_audio.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/engine_audio.rs).

---

## 6. Audit & Verification Gate

- [x] **6.1. Workspace Unit Tests**
  - `cargo test --release --workspace` passes 51/51 tests with zero failures across all 4 crates.
- [x] **6.2. Asset Validation**
  - `mxb-dump validate` validates all 691 stock game assets across all 18 tracks with zero failures.
- [x] **6.3. Physics Telemetry Simulation**
  - `mxb-dump sim 250xf` runs clean across all scenarios:
    - Launch and acceleration.
    - Riderless self-stability at 30 km/h and 50 km/h with 0.3 rad/s roll-rate kick.
    - Steer step inputs at 30 km/h, 60 km/h, and 110 km/h.
    - Emergency stop with dual brakes and clutch in (stopped from 122.2 km/h in 69.8 m).

---

## 7. Graphics Engine Functions & Dynamic Window Reconfiguration

- [x] **7.1. Graphics Settings Dynamic Propagation (`FUN_1400d4580`, `ApplyGraphicsSettings`)**
  - **Source:** `FUN_1400d4580` (message handler for `idd_graphic_options`), `FUN_1400ec640` (options apply/exit).
  - **Resolution Stepping:** `Profile::spin` cycles standard display resolutions (`800x600` through `3840x2160`) updating `gfx.x` and `gfx.y`.
  - **Refresh Rate & BPP Stepping:** `Profile::spin` cycles refresh rates (`0` default, `60`, `75`, `120`, `144`, `165`, `240`, `360`) and BPP (`16`, `32`).
  - **Live Window Reconfiguration:** Clicking `id_done` emits `ApplyGraphicsSettings`:
    - Updates `Window::resolution` dynamically without restarting the application.
    - Updates `Window::mode` between Windowed, Fullscreen, and Borderless Fullscreen (`gfx.fullscreen`).
    - Updates `Window::present_mode` for VSync toggling (`gfx.vsync`).
    - Updates `DirectionalLight::shadows_enabled` based on `gfx.shadow_disable`.
    - Updates `Camera3d` `Msaa` component based on `gfx.multisample` (`Off`, `Sample2`, `Sample4`, `Sample8`).
    - Updates camera `PerspectiveProjection::fov` dynamically from `view.fov`.
  - **Terrain Shading PBR Conformity:** `terrain.wgsl` sets `perceptual_roughness = 0.92` and `metallic = 0.0` matching PiBoSo's non-glossy soil reflectance.
  - **Files:** [`crates/open-mxbikes/src/profile.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/profile.rs), [`crates/open-mxbikes/src/ui.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/ui.rs), [`crates/open-mxbikes/src/main.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/main.rs), [`crates/open-mxbikes/src/shaders/terrain.wgsl`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/shaders/terrain.wgsl).

---

## 8. Complete Menu & UI Buttons Parity

- [x] **8.1. Full Button & Action Dispatch Matrix (`ui/*.mnu`)**
  - **Source:** Decompiled message dispatchers `FUN_1400e2640` (testing pit), `FUN_1400a6680` (garage), `FUN_1400b3350` (profiles), `FUN_1400eb870` (viewreplays), `FUN_140119970` (testsetup).
  - **Testing Pit (`test.mnu`):**
    - `id_start`: transitions to track.
    - `id_garage`: opens garage setup menu.
    - `id_settings`: opens options/settings menu.
    - `id_replay`: opens replay browser.
    - `id_photo`: pit photo mode.
    - `id_topit`: sends bike to pit stall (`ToPits(true)`).
    - `id_restart`: lap reset and teleport to pit stall.
    - `id_stand`: toggles pit stand.
    - `id_bikeclean` / `id_resettrack`: resets chassis and rider dirt factors to 0.0.
    - `id_back`: prompts `idd_testingexit`, returning to `MainMenu` on `id_yes`.
  - **Garage (`garage.mnu`):**
    - `id_done`: applies setup (`ApplySetup(true)`) and returns to pit.
    - `id_undo`: resets setup values to stock default.
    - `id_save` / `id_saveas`: saves binary `.stp` setup with name validation.
    - `id_delete`: prompts `idd_setup_delete` and deletes `.stp` file.
    - `id_copyfrom`: opens copy setup selector dialog.
    - `id_notes`: opens `idd_setup_notes` editbox.
    - `id_bikeclean`: cleans bike dirt factor.
    - `id_fronttyresetreset` / `id_reartyresetreset`: resets tyre wear.
  - **Profiles (`profiles.mnu`):**
    - `id_done`: saves profile and returns to main menu.
    - `id_new` / `id_modify`: opens `idd_profilemodify`.
    - `id_delete`: opens `idd_profiledeleteconfirm`.
    - `id_guidcopy`: copies profile GUID.
  - **Replays (`viewreplays.mnu`):**
    - `id_back`: returns to menu.
    - `id_view`: opens replay viewer mode.
    - `id_delete`: opens `idd_confirm_replay_delete`.
  - **File:** [`crates/open-mxbikes/src/ui.rs`](file:///n:/etc/rewrite%20project/crates/open-mxbikes/src/ui.rs).
