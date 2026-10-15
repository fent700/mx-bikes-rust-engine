# PiBoSo asset formats (MX Bikes beta21)

Reverse-engineered from the stock content. Every rule below is enforced by `mxb-dump validate`,
which parses all stock assets; the status column reports what currently parses.

All binary data is little-endian. Assets are authored in a **left-handed** frame: +X right,
+Y up, +Z forward. The engine mirrors Z on load (`reader::lh_to_rh`) and flips triangle winding.

| Format | Contents | Parser | Stock coverage |
|---|---|---|---|
| `.pkz` | plain ZIP archive | `mxbikes-vfs` | 8/8 |
| `.cfg .ini .geom .engn .tyre .hrc .amb .rdf .tsc` | key/value block text | `piboso_tree` | all |
| `.anm` | skeletal animation tracks (quaternions + translations) | `anm.rs` | 92/92 |
| `.edf` | scenes: meshes, node hierarchy, materials, embedded textures | `edf.rs` + `texture.rs` | 104/104 |
| `.map` (`MP2\0`, legacy `MAP\0`) | track scenery, collision, terrain layers | `map.rs` | 16/16 (15 tracks + menu background) |
| `.trh` | terrain heightfield + surface table | `trh.rs` | 18/18 |
| `.tl` | track lines (long/lat) | `tl.rs` | 54/54 |
| `.pnt` | paints: replacement diffuse textures | `pnt.rs` | 61/61 |
| `.mnu`, `ui.ui` | menu layouts | `mnu.rs` | 16/16 |
| `.str` | UI string table | `mnu.rs` | 1/1 |
| `.fnt` | bitmap fonts | `fnt.rs` | 7/7 |
| `.tga` | sprites, fonts, particle textures (types 2 / 10; 16, 24, 32 bpp) | `tga.rs` | 267/267 |
| `.stp` | binary garage setup container | `setup_file.rs` | 100% specification verified |

Every stock asset parses (`mxb-dump validate`). Each paint layer is a complete material with its
own shading block, and the list is counted rather than zero-terminated. The supermoto tyre is the
stock file that exposed this: its second layer starts with a 0 word. `misc/bg/bg.map` is the menu
background in the older `MAP\0` layout, which is the `MP2` scenery mesh and material table without
a version word or terrain block.

## EDF

```
"EDF\0" bbox_min:vec3 bbox_max:vec3
root objects                         until footer.root_end (one object when there is no footer)
material table                       count:u32 Material*count
named groups                         bbox:vec3*2 objects ...      (offsets from the footer)
footer (optional)                    root_end:u32 count:u32 {name[100] offset:u32}*count "EXT\0" footer_size:u32
```

Named groups are the parts the `.hrc` files reference (`rsuspa`, `steera`, `fsuspa`, `chassisb`,
...). LOD variants are separate groups.

**Object**

```
material_count:u32 { flags:u32 color:f32*6 slots:u32*7 }*n   slots[4] = 1-based material table index (0 = untextured)
vertex_count:u32
structure of arrays: position f32*3 | uv0 f32*2 | uv1 f32*2 | color f32*4 | normal f32*3 | tangent f32*4   (72 B/vertex)
triangle_count:u32 indices:u32*3*n
node_count:u32 Node*n
```

**Node**

```
name[100] (0xCD padded)  flags:u32
transform f32*16  row-major, translation in [3],[7],[11], parent-relative
pivot     f32*16
parent_plus_one:u32 (0 = root)  child_count:u32 children:u32*n
submesh_count:u32 { kind material first_tri tri_count first_vertex vertex_count }*n (24 B/submesh)
node_vertex_count:u32 reserved:u32 (always present, 8 B)
flags & 0x2000: skinned bone weights (node_vertex_count * 124 bytes)
flags & 0x10000: morph_count:u32 { frames:u32 position:vec3[v*frames] normal:vec3[v*frames]
                 tangent:vec4[v*frames] time:f32[frames] }*morph_count   (v = node_vertex_count)
bbox vec3*2
```

The morph sets are baked per-frame vertex animation (the cloth flags carry four sets of up to 25
frames). This layout and the unconditional vertex-count pair match the game's EDF loader.

## ANM (Skeletal Animation)

```
"ANM\0" version:u32 (1 or 2)
animation clips until EOF:
  name[100] frame_count:u32 bone_count:u32
  {
    bone_name[100]
    { quat:f32*4 (x,y,z,w) translation:f32*3 (x,y,z) } * frame_count  (28 B/frame)
    weight:f32    per-bone mask, 0.0 or 1.0 in every stock file
  } * bone_count
```

The per-bone weight is the mask the game's pose evaluator multiplies into the clip weight. It
matches what each clip animates: `throttle` masks in only the right wrist and fingers,
`head lean sitting` only the neck and head, and `leaning sitting` everything else.

Checked on all 92 stock files: every quaternion is unit length and every file is consumed
exactly. Reading a `flags:u32` before the keys instead shifts the keys by one word and makes
about half of the quaternions non-unit. Keys are full parent-relative transforms: their
translations equal the EDF node rest offsets. The stored quaternion is the inverse of the EDF
rotation convention, so conversion to the right-handed engine frame is quat (x, y, -z, w) and
translation (x, y, -z). The `rider.edf` pelvis rest rotation is reproduced exactly.

## Material table (EDF and MAP)

```
Material:
  diffuse: FullImage(with flags)
  shading:u32 (0 unlit/simple, 1 lit, 2 lit + extra)
  shading != 0:
    has_reflection:u32 -> params:f32*3 has_map:u32 [MapImage] has_env:u32 [ShortImage]
    has_specular:u32   -> power:f32 has_map:u32 [MapImage]
    has_normal:u32     -> strength:f32 FullImage(no flags)
  shading == 2: has_extra:u32 [f32*3] mode:u32
  paint layers (objects only, not terrain variants):
    count:u32 { flags:u32 color:f32*6 slots:u32*7 Material(no paint layers) }*count

MapImage: ShortImage, or FullImage(no flags) when shading == 2

FullImage:  name[100] [flags:u32] width:u32 height:u32 hash[16] extra:u32 hash[16]*extra Blob*(extra+1)
ShortImage: name[100] width:u32 height:u32 hash[16] Blob
Blob:       size:u32 { format:u32 reserved:u32 raw-deflate } (size 0 = empty frame)
```

| format | texels | used by |
|---|---|---|
| 0 | RGBA8 | diffuse, normal maps |
| 2 | L8 (1 channel) | reflection/specular maps, terrain masks |
| 5 | RGB8 cubemap, 6 faces | `env` reflection maps |

Image flags: bit 0, 0x100, 0x400 mark alpha-tested foliage; 0x2 marks paintable (number plates).

## MAP (`MP2\0`)

```
"MP2\0" version:u32 (288 or 304)
scenery: object material slots, vertex_count, SoA vertices (80 B: EDF layout + uv2 f32*2 after uv1),
         triangles, AABB tree node_count {bbox f32*6 left+1 right+1, inner: u32*3 | leaf: tri_count base runs
         {flags material first_tri tri_count first_vertex vertex_count}*runs}
material table (as EDF)
collision mesh + spatial tables (not needed for rendering)
terrain block: size_x size_z height:f32 origin:vec3 layer_count:u32
  layer: slot(56 B) variant_count:u32 Material(no paint layers)*variant_count
         tiling:f32 (version 304: tiling_u tiling_v) has_mask:u32 [width height Blob] surface_param:f32
```

Variant 0 is the dry surface, variant 1 the wet one. Layer 0 is the base. Each later layer
blends over the stack by `mask / 255` across the terrain extent.

## TRH

```
"TRH\0" width:u32 depth:u32 heights:i16*width*depth   row j = asset Z, column i = asset X
size_x:f32 height_range:f32 size_z:f32 origin:vec3
surface_count:u32 {surface_id:u32 deform_depth:f32 mask_w:u32 mask_h:u32 mask:u8*w*h}*count
extra maps (wear / blend, kept raw)
centreline: start_x:f32 start_z:f32 heading_deg:f32, 4 + 24 B unknown,
            surface names (52 B records), segments {kind:u32 length:f32 radius:f32, 48 B}*n
EXT / EX3 extra track lines
```

The per-layer masks are the physics surface masks: layer 0 is the base surface and each later
layer overrides it where `mask >= 128`. The `.map` terrain blend masks are only a fallback for
layers that have no TRH mask.

### Centreline

Every `long`/`lat` track coordinate (`.rdf` gates, checkpoints and `pit_lane/start_stallN`,
`.tl` lines) is measured along this line. Segments are straights (`kind 0`) or arcs (`kind 1`,
signed radius). A running 2D transform starts at (start_x, start_z) rotated by `heading_deg`.
Straights advance it. Arcs rotate it by `length / (2π|r|) · 360°`, negated for `r <= 0`. Inside
a segment, (long, lat) is local (lat, long) on a straight and polar about the centre on an arc.
Closed circuits wrap `long`. Point-to-point lines clamp it.
The engine classifies a track as open/point-to-point (`FUN_1401f3df0` lines 26-34) if the Euclidean
distance between the end of the last segment and the line's start position is greater than `0.01 m`
(1 cm), or if the total length is `< 0.1 m`. Assen's authored centreline leaves a 13.54 m gap at its close,
so the engine classifies it as an open track. Straight Rhythm has an end-to-start gap of 684.55 m and is
likewise open. All other 16 stock tracks close with a gap of 0.00 m.

The game spawns the bike at `pit_lane/start_stall0` mapped through the centreline (`FUN_1401027b0`), with
the absolute heading `atan2(dx, dz)` of the line plus the stall `angle`. The `.rdf` fields `startposx/z`
and `startangleabs` are editor caches from the PiBoSo Track Editor and are ignored by the game engine.

`y = origin.y + (h + 32768) / 65535 * height_range`. This was verified against scenery ground
contact (median scenery vertex 0.68 m above terrain) and the `.tsc` camera positions (the pit
camera sits 1.6 m above ground). TRH surface ids use the game's surface enumeration (confirmed from its tyre loader): 0 asphalt1, 1 asphalt2, 2 asphalt3, 3 concrete, 4 grass, 5 sand, 6 kerb, 7 soil, 8 paint, 9 artificial turf, 10 soft soil, 11 compact soil, 12 gravel, 13 rock. Compound `bulldozing_factor` is scaled by 10000 on load.

## PNT

```
"PNT\0" name[100] count:u32 ShortImage*count
```

Each image replaces the model diffuse texture with the same name: `livery`, `onboard` and
`wheels` for bikes (the tyre models use `wheels` too), `rider` and `gloves` for the suit, `helmet`,
`goggles` and `visor` for helmets, and `boots`. The bike `official` paint only carries `wheels`,
because the embedded livery is the official one. Paths follow the game: `bikes/<id>/paints`,
`rider/riders/<rider>/paints` and `/gloves`, `rider/helmets/<helmet>/paints` and `/goggles`,
`rider/boots/<boots>/paints`.

The single paint layer on each paintable material is a mud texture with alpha (flags `0x2`).
The game blends it in as dirt builds up (the `dirt_color` keys in the gear configs), so it is not
part of the paint.

## MNU / STR / FNT

`.mnu` is plain text: `key value ...` lines and `name` blocks in braces, no comments. Items
(`item_button`, `item_text`, `item_bitmap`, `item_darkbox`, ...) give `rect x0 y0 x1 y1` in screen
fractions of the 16:9 layout `ui.ui` declares, colours as `a r g b`, and a `text` block with `font`,
`pos`, `fontsize` (screen-height fraction) and `align`. A file holds several `dialog`s (main page
plus popups). `english.str` alternates id and text lines.

```
FNT: "FNT\0" name[260] size:u32
     256 x {exists a b c x0 x1 y0 y1 kern_start kern_count : i32}
     kern_count:u32 {second_char:i32 amount:i32}*n
     width:u32 height:u32 blob_size:u32 Blob(format 2 = L8 coverage)
```

Glyphs use ABC widths (advance a + b + c); the game samples `x0 .. x1 + 2` and `y0 .. y1`
(`FUN_140208e70`).

## TL

`"TL \0" count:u32 {long:f32 lat:f32}*count`: points in track coordinates (distance along the
centreline, lateral offset).

## Physics data units

* `.engn` curve x is crank speed in **rev/s** (×60 = rpm), y is torque in N·m. At `IdleThrottle`
  the combustion and `brake` curves balance at `IdleRPM`, which confirms
  `torque = full·t + brake·(1 − t)`.
* Gearing pairs `a, b` are tooth counts: ratio = b / a. `Ratio0` in `engine` is the primary drive.
* Setup blocks resolve as `range = min, step, max` + `setting = n` → `min + n·step`.
  Indexed tables (`setting0..N`, `gear0..N`) are selected by the sibling `*Setting` key.
* Dampers: `BumpThreshold0/1` (m/s) split three linear stages. `settingN = c0, c1, c2` (N·s/m).
* Brake torque = `RiderMaxForce · leverage / master-cylinder area` (kPa) × `PressureToTorque`.
* `.tyre` files carry Pacejka MF-MC (motorcycle) coefficients, including the camber-thrust terms
  `p_Cy2`, `p_Ky6`, `p_Ky7` and `p_Ey5`.
* Bike tyres use the MF-MC terms as the game evaluates them:
  `μy = p_Dy1·exp(p_Dy2·dfz) / (1 + p_Dy3·γ²)`,
  `Kya = p_Ky1·Fz0·sin(p_Ky2·atan(Fz / ((p_Ky3 + p_Ky4·γ²)·Fz0))) / (1 + p_Ky5·γ²)`,
  `Kyγ = (p_Ky6 + p_Ky7·dfz)·Fz`, `Ey = p_Ey1 + p_Ey2·γ² + sgn(αy)(p_Ey3 + p_Ey4·γ)`,
  `αy = α + p_Hy1`, and a rolling-resistance slip shift `κx = κ − q_sy1·Fz / Kx`.
  The other `.tyre` evaluator in the exe belongs to the `type = car` path and is not used for
  bikes.
* `gearbox/autoshift/gearN { down up }` is the rpm table for the `autoshift` rider aid, a
  per-profile `profile.ini` option next to `autoclutch` and `leanhelp`. It is off unless enabled.

## PRM (`rider.prm`)

Encrypted parameter container stored in `rider.pkz!rider/rider.prm`.
Loaded by engine function `FUN_140192ec0` via container decryptor `FUN_140157920`.

```
encrypted payload: u8*length
12-byte footer:
  magic:   u32 = 0x43525950 ("PYRC" in little-endian)
  seed:    u32 = 0x201 (513 in stock rider.prm)
  length:  u32 = total_file_size - 12 (1316 bytes in stock rider.prm)
```

* **Cipher:** ARC4 (RC4) stream cipher.
* **Master Key:** 16 bytes `b"XL$S$udrla#!oaSW"` (from literal `0x7264752453244c58` + `"la#!oaSW"`).
* **Key Scheduling:** KSA with master key, followed by discarding `seed + 0x100` bytes (769 bytes for stock seed 0x201) (`FUN_14015af80`).
* **Decryption:** PRGA decrypt of payload bytes (`FUN_14015b070`).
* **Payload:** PiBoSo block tree ASCII text declaring rider spring and slew parameters:
  - `LeanSpeed` (2.0): torso lean slew rate (+0x23780)
  - `PitchSpeed` (5.0): fore/aft pitch slew rate (+0x23784)
  - `PitchSpring/SeatForward` (0.1), `PitchSpring/SeatBackward` (0.2), `PitchSpring/StandingForward` (0.5), `PitchSpring/StandingBackward` (0.5)
  - `RotationSpring` (2.0), `LongSpring` (9000.0), `SpringsFactor` (3.0), `LongDamping` (0.6)
  - `Legs/SeatSpring` (200.0), `Legs/StandSpring` (400.0), `Legs/CompDamping` (1.0), `Legs/ExtDamping` (2.5)
  - `Standing/Pos` (0.1), `Stability/Pitch/Factor` (0.5), `Stability/Yaw/Factor` (0.22), `Stability/Roll/Factor` (0.25)

## Engine Physics Timestep & Ruts

* **Fixed physics rate:** The engine's fixed physics timestep is **500 Hz** (`dt = 0.002` s, 2 ms).
  `DAT_140396b48 = 2` is initialized in `FUN_140133e80` by calling `FUN_1401b9520(2, FUN_140120590)`.
  The main simulation dispatch loop `FUN_1401be8a0` runs at fixed 500 Hz (`(float)DAT_140396b48 * 0.001 = 0.002` s per substep).
* **Rut deformation (`FUN_1401a2fb0` / `FUN_1401f5ac0`):**
  - Contact velocity $v$: differences contact position between steps: `v = length(pt_now - pt_prev) / dt`. If $v < 0.1$ m/s, $v = 0.0$.
  - Deformation amount: $v \cdot \min(F, 5000) \cdot 4\times 10^{-5} \cdot dt$.
  - Deform threshold: $0.1$ mm (`1e-4` m) is subtracted per call in `FUN_1401f5ac0` line 54 before surface factor scaling and bilinear splat.

## Roost Particles (`FUN_140129a20`)

* **Emission:**
  - Interval: `1.0 / (frequency / (wheel_speed / size_speed_scale))` seconds.
  - Cone distribution: spherical polar coordinates with polar angle $\theta = \text{rand}(0, 1) \cdot \text{exit\_variance} \cdot \frac{\pi}{180}$ and azimuth $\phi = \text{rand}(0, 1) \cdot 2\pi$.
* **Analytical Closed-Form Drag Ballistics (`FUN_140129a20` lines 85-103):**
  The engine integrates linear drag $F_{drag} = -\text{friction} \cdot v$ and gravity $g$ analytically:
  $$v_x(t) = v_{0x} e^{-\text{friction}\cdot t}, \quad v_z(t) = v_{0z} e^{-\text{friction}\cdot t}$$
  $$v_y(t) = \left(v_{0y} - \frac{g}{\text{friction}}\right) e^{-\text{friction}\cdot t} + \frac{g}{\text{friction}}$$
  $$x(t) = x_0 + \frac{v_{0x}}{\text{friction}} (1 - e^{-\text{friction}\cdot t})$$
  $$z(t) = z_0 + \frac{v_{0z}}{\text{friction}} (1 - e^{-\text{friction}\cdot t})$$
  $$y(t) = y_0 + \left(v_{0y} - \frac{g}{\text{friction}}\right) \frac{1}{\text{friction}} (1 - e^{-\text{friction}\cdot t}) + t \frac{g}{\text{friction}}$$

## Rider Detachment / Crash Mechanics (`FUN_1401a8480`)

* **Fall Reference Point:** authored in `rider/rider.cfg` as `fall_ref = 0, -0.2, -0.05` (LH frame, relative to chassis rider point).
* **Fall Radius:** $0.31$ m for bikes (`fVar37 = 0.31` in `FUN_1401a8480` line 120).
* **Detachment Trigger:** when $(fall\_ref\_world.y - \text{ground\_height}) \le 0.31$, `FUN_1401a79f0` is invoked and rider detaches from the bike.
* **Force Buffer Invariant:** 25-frame rolling reaction force window triggers detachment if $|F_x| > 4000\text{ N}$, $|F_z| > 10000\text{ N}$, $F_y > 15000\text{ N}$, or moving average $F_{y\text{ ext}} < -2000\text{ N}$ (`FUN_1401a8480` lines 1489–1499).

## Garage Setup Container (`.stp`)

* **Engine Operations:** `0x23b` (saver: `FUN_140199330`), `0x23c` (loader: `FUN_140198e10`), validator `FUN_140199e00`.
* **Path Hierarchy (`FUN_14009bf20`):**
  - Track-specific setup: `profiles/<profile>/setups/<track>/<bike>/<name>.stp`
  - Common bike setup: `profiles/<profile>/setups/common/<bike>/<name>.stp`
  - Bike default: `bikes/<bike>/setups/<name>.stp`
* **Binary Specification:**
  - `0x00..0x04`: Magic `0x459353c8` (u32 little-endian)
  - `0x04..0x06`: Version `0x116` or `0x115` (u16 little-endian)
  - `0x06..0x26`: Bike ID / GUID (32 bytes ASCII null-padded)
  - `0x26..0x28`: Bike subversion (u16 little-endian)
  - `0x28..0x3c`: 5 x u32 (tyre compound front/rear, tyre pressure front/rear, fuel)
  - `0x3c..0x74`: 14 x u32 (suspension springs and dampers)
  - `0x74..0x78`: 1 x u32 (final drive rear sprocket)
  - `0x78..`: `num_gears` x u32 (sequential gear ratios)
  - Followed by secondary driveline (2 words), tyre parameters (6 words), chassis geometry (16 words).
  - Variable-length trailing notes string (u32 length prefix followed by ASCII characters).



