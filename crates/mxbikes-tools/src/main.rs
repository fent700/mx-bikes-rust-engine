//! `mxb-dump`: inspection and validation for MX Bikes assets mounted from a local install.
//!
//!   mxb-dump validate                 parse every supported asset and report failures
//!   mxb-dump edf <vfs path> [out]     summarise an .edf; export textures (png) and root mesh (obj)
//!   mxb-dump bike <id>                print the resolved bike definition as JSON
//!   mxb-dump track <name> [out]       summarise terrain + scenery; export terrain layers and masks

use glam::Vec3;
use mxbikes_formats::assets;
use mxbikes_formats::map::TrackMap;
use mxbikes_formats::texture::DecodedImage;
use mxbikes_formats::{AnimationFile, EdfModel, FontFile, MenuNode, PaintFile, SoundCurveLayout, StringTable, TrackLine, TrackTerrain, TyreDefinition};
use mxbikes_vfs::MxBikesVfs;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let vfs = match std::env::var("MXB_GAME_DIR") {
        Ok(dir) => MxBikesVfs::open(dir),
        Err(_) => MxBikesVfs::auto_detect(),
    }
    .unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(2);
    });

    let code = match args.first().map(String::as_str) {
        Some("validate") => validate(&vfs),
        Some("edf") if args.len() >= 2 => dump_edf(&vfs, &args[1], args.get(2).map(PathBuf::from)),
        Some("bike") if args.len() >= 2 => dump_bike(&vfs, &args[1]),
        Some("track") if args.len() >= 2 => dump_track(&vfs, &args[1], args.get(2).map(PathBuf::from)),
        Some("sim") => sim(&vfs, args.get(1).map(String::as_str).unwrap_or("250xf")),
        Some("ride") => ride(&vfs, args.get(1).map(String::as_str).unwrap_or("club")),
        _ => {
            eprintln!("usage: mxb-dump validate | edf <path> [out] | bike <id> | track <name> [out] | sim [bike]");
            2
        }
    };
    std::process::exit(code);
}

fn validate(vfs: &MxBikesVfs) -> i32 {
    let mut files = vfs.list_files("");
    files.sort();
    let mut stats: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
    let mut failures = Vec::new();
    let start = Instant::now();

    for path in &files {
        let ext = path.rsplit('.').next().unwrap_or("");
        let result: Result<(), String> = match ext {
            "edf" => vfs
                .read(path)
                .map_err(|e| e.to_string())
                .and_then(|d| EdfModel::parse(&d).map(|_| ()).map_err(|e| e.to_string())),
            "anm" => vfs
                .read(path)
                .map_err(|e| e.to_string())
                .and_then(|d| AnimationFile::parse(&d).map(|_| ()).map_err(|e| e.to_string())),
            "trh" => vfs
                .read(path)
                .map_err(|e| e.to_string())
                .and_then(|d| TrackTerrain::parse(&d).map(|_| ()).map_err(|e| e.to_string())),
            "tl" => vfs
                .read(path)
                .map_err(|e| e.to_string())
                .and_then(|d| TrackLine::parse(&d).map(|_| ()).map_err(|e| e.to_string())),
            "map" => validate_map(vfs, path),
            "mnu" | "ui" => vfs
                .read(path)
                .map_err(|e| e.to_string())
                .and_then(|d| MenuNode::parse(&String::from_utf8_lossy(&d)).map(|_| ())),
            "tga" => vfs
                .read(path)
                .map_err(|e| e.to_string())
                .and_then(|d| mxbikes_formats::tga::decode_tga(&d).map(|_| ()).ok_or_else(|| "unsupported TGA".to_string())),
            "fnt" => vfs.read(path).map_err(|e| e.to_string()).and_then(|d| FontFile::parse(&d).map(|_| ()).map_err(|e| e.to_string())),
            "str" => vfs.read(path).map_err(|e| e.to_string()).and_then(|d| {
                let t = StringTable::parse(&String::from_utf8_lossy(&d));
                if t.is_empty() { Err("empty string table".into()) } else { Ok(()) }
            }),
            "pnt" => vfs.read(path).map_err(|e| e.to_string()).and_then(|d| {
                let p = PaintFile::parse(&d).map_err(|e| e.to_string())?;
                p.textures
                    .iter()
                    .try_for_each(|t| t.decode_frame(&d, 0).map(|_| ()).map_err(|e| format!("{}: {e}", t.name)))
            }),
            "tyre" => assets::read_cfg(vfs, path)
                .map(|n| {
                    let t = TyreDefinition::from_node(&n);
                    if t.mf.len() < 20 {
                        Err(format!("only {} MF coefficients", t.mf.len()))
                    } else {
                        Ok(())
                    }
                })
                .unwrap_or_else(|| Err("unparseable".into())),
            "scl" => assets::read_cfg(vfs, path)
                .map(|n| {
                    let s = SoundCurveLayout::parse(&n);
                    if s.layers.is_empty() {
                        Err("empty layers".into())
                    } else {
                        Ok(())
                    }
                })
                .unwrap_or_else(|| Err("unparseable".into())),
            "prm" => vfs
                .read(path)
                .map_err(|e| e.to_string())
                .and_then(|d| mxbikes_formats::RiderPrm::parse(&d).map(|_| ()).map_err(|e| e.to_string())),
            // Sounds are PYRC containers; the VFS read must yield a RIFF/WAVE file.
            "wav" => vfs.read(path).map_err(|e| e.to_string()).and_then(|d| {
                if d.len() > 12 && &d[..4] == b"RIFF" && &d[8..12] == b"WAVE" {
                    Ok(())
                } else {
                    Err("not a RIFF/WAVE file after decryption".into())
                }
            }),
            _ => continue,
        };
        let entry = stats.entry(match ext {
            "anm" => "anm",
            "edf" => "edf",
            "trh" => "trh",
            "tl" => "tl",
            "map" => "map",
            "pnt" => "pnt",
            "mnu" | "ui" => "mnu",
            "fnt" => "fnt",
            "tga" => "tga",
            "str" => "str",
            "scl" => "scl",
            "prm" => "prm",
            "wav" => "wav",
            _ => "tyre",
        });
        let e = entry.or_default();
        match result {
            Ok(()) => e.0 += 1,
            Err(msg) => {
                e.1 += 1;
                failures.push(format!("{path}: {msg}"));
            }
        }
    }

    for id in assets::bike_ids(vfs) {
        let e = stats.entry("bike").or_default();
        match assets::load_bike(vfs, &id) {
            Ok(b) if !b.engine.torque_curve.is_empty() && !b.gearbox.ratios.is_empty() => e.0 += 1,
            Ok(_) => {
                e.1 += 1;
                failures.push(format!("bike {id}: missing torque curve or gears"));
            }
            Err(err) => {
                e.1 += 1;
                failures.push(format!("bike {id}: {err}"));
            }
        }
    }

    // Centreline check: stall 0 through the TRH centreline must land on the editor start pose.
    for stem in assets::track_paths(vfs) {
        let Ok(Ok(t)) = vfs.read(&format!("{stem}.trh")).map(|d| TrackTerrain::parse(&d)) else { continue };
        let Some(line) = &t.centerline else {
            failures.push(format!("{stem}: no centreline"));
            continue;
        };
        let Some(g) = assets::load_track_definition(vfs, &stem).and_then(|d| d.start) else { continue };
        let Some(st) = g.stalls.first() else { continue };
        let p = line.to_world(st.long, st.lat);
        let err = p.distance(glam::Vec2::new(g.pos_x, g.pos_z));
        let dh = ((line.heading_deg(st.long) + st.angle_deg - g.angle_deg + 540.0) % 360.0) - 180.0;
        println!(
            "  centreline {:<55} {} segs {:7.1} m {}  stall0 offset {:6.2} m  heading diff {:6.2} deg",
            stem,
            line.segments.len(),
            line.length,
            if line.open { "open  " } else { "closed" },
            err,
            dh
        );
    }

    for f in &failures {
        println!("FAIL {f}");
    }
    for (k, (ok, bad)) in &stats {
        println!("{k:>5}: {ok:4} ok {bad:4} failed");
    }
    println!("validated in {:.1}s", start.elapsed().as_secs_f32());
    if failures.is_empty() {
        0
    } else {
        1
    }
}

fn terrain_extent(vfs: &MxBikesVfs, map_path: &str) -> Option<(f32, f32, f32)> {
    let stem = map_path.strip_suffix(".map")?;
    let t = TrackTerrain::parse(&vfs.read(&format!("{stem}.trh")).ok()?).ok()?;
    Some((t.size_x, t.size_z, t.height_range))
}

fn validate_map(vfs: &MxBikesVfs, path: &str) -> Result<(), String> {
    let data = vfs.read(path).map_err(|e| e.to_string())?;
    let map = TrackMap::parse(&data, terrain_extent(vfs, path)).map_err(|e| e.to_string())?;
    if map.version != 0 && map.terrain.is_none() {
        return Err("terrain material block not found".into());
    }
    Ok(())
}

fn save_png(img: &DecodedImage, path: &Path) {
    let h = img.height * img.layers;
    if let Some(buf) = image::RgbaImage::from_raw(img.width, h, img.rgba.clone()) {
        if let Err(e) = buf.save(path) {
            eprintln!("  could not write {}: {e}", path.display());
        }
    }
}

fn dump_edf(vfs: &MxBikesVfs, path: &str, out: Option<PathBuf>) -> i32 {
    let data = match vfs.read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let model = match EdfModel::parse(&data) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("parse failed: {e}");
            return 1;
        }
    };
    println!("{path}: bbox {:?} .. {:?}", model.bbox_min, model.bbox_max);
    for g in &model.groups {
        let name = if g.name.is_empty() { "<root>" } else { &g.name };
        for o in &g.objects {
            println!(
                "  group {name}: {} verts, {} tris, materials {:?}",
                o.vertices.len(),
                o.indices.len() / 3,
                o.materials.iter().map(|m| m.texture).collect::<Vec<_>>()
            );
            for (i, n) in o.nodes.iter().enumerate() {
                let t = o.node_world(i).w_axis.truncate();
                let morph_info = if !n.morphs.is_empty() {
                    format!(" morphs {} ({} frames, duration {:.2}s)", n.morphs.len(), n.morphs[0].frames, n.morphs[0].times.last().copied().unwrap_or(0.0))
                } else {
                    String::new()
                };
                println!(
                    "    node {i:2} {:<20} parent {:?} subs {} at {:.3?}{}",
                    n.name,
                    n.parent,
                    n.submeshes.len(),
                    t,
                    morph_info
                );
            }
        }
    }
    for (i, m) in model.materials.iter().enumerate() {
        println!(
            "  material {}: {} {}x{} flags {:#x} shading {} refl {} spec {} normal {} paint {}",
            i + 1,
            m.diffuse.name,
            m.diffuse.width,
            m.diffuse.height,
            m.diffuse.flags,
            m.shading,
            m.reflection.as_ref().and_then(|r| r.map.as_ref()).map(|i| i.name.as_str()).unwrap_or("-"),
            m.specular.as_ref().and_then(|s| s.map.as_ref()).map(|i| i.name.as_str()).unwrap_or("-"),
            m.normal.as_ref().map(|n| n.map.name.as_str()).unwrap_or("-"),
            m.paint_layers.len()
        );
    }

    if let Some(dir) = out {
        std::fs::create_dir_all(&dir).ok();
        for (i, m) in model.materials.iter().enumerate() {
            match m.diffuse.decode_frame(&data, 0) {
                Ok(img) => save_png(&img, &dir.join(format!("{:02}_{}.png", i + 1, m.diffuse.name))),
                Err(e) => eprintln!("  texture {}: {e}", m.diffuse.name),
            }
        }
        if let Some(root) = model.groups.first().and_then(|g| g.objects.first()) {
            let obj = dir.join("root.obj");
            if let Ok(mut f) = std::fs::File::create(&obj) {
                let mut world = vec![Vec3::ZERO; root.vertices.len()];
                for (ni, n) in root.nodes.iter().enumerate() {
                    let m = root.node_world(ni);
                    for s in &n.submeshes {
                        for v in s.first_vertex..s.first_vertex + s.vertex_count {
                            world[v as usize] = m.transform_point3(root.vertices[v as usize].position);
                        }
                    }
                }
                for p in &world {
                    writeln!(f, "v {} {} {}", p.x, p.y, p.z).ok();
                }
                for t in root.indices.chunks_exact(3) {
                    writeln!(f, "f {} {} {}", t[0] + 1, t[1] + 1, t[2] + 1).ok();
                }
                println!("  wrote {}", obj.display());
            }
        }
    }
    0
}

fn dump_bike(vfs: &MxBikesVfs, id: &str) -> i32 {
    match assets::load_bike(vfs, id) {
        Ok(b) => {
            let (kw, rpm) = b.peak_power();
            eprintln!(
                "{}: peak {:.1} kW @ {:.0} rpm, front brake {:.0} Nm, rear brake {:.0} Nm, gear1 overall {:.2}",
                b.name,
                kw,
                rpm,
                b.brakes[0].max_torque(),
                b.brakes[1].max_torque(),
                b.overall_ratio(1)
            );
            println!("{}", serde_json::to_string_pretty(&b).unwrap());
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn dump_track(vfs: &MxBikesVfs, name: &str, out: Option<PathBuf>) -> i32 {
    let Some(stem) = assets::find_track(vfs, name) else {
        eprintln!("track '{name}' not found; known: {:?}", assets::track_paths(vfs));
        return 1;
    };
    let terrain = match vfs.read(&format!("{stem}.trh")).map(|d| TrackTerrain::parse(&d)) {
        Ok(Ok(t)) => t,
        other => {
            eprintln!("terrain: {other:?}");
            return 1;
        }
    };
    println!(
        "{stem}: terrain {}x{} samples over {}x{} m, height range {} m, origin {:?}",
        terrain.width, terrain.depth, terrain.size_x, terrain.size_z, terrain.height_range, terrain.origin
    );
    for s in &terrain.surfaces {
        println!("  surface id {} ({:?}) deform {}", s.surface_id, mxbikes_formats::Surface::from_terrain_id(s.surface_id), s.deform_depth);
    }
    if let Some(def) = assets::load_track_definition(vfs, &stem) {
        println!("  {} ({} m), start {:?}", def.name, def.length, def.start);
    }
    let Some(map_path) = assets::track_map_path(vfs, &stem) else {
        return 0;
    };
    let data = vfs.read(&map_path).unwrap();
    let map = match TrackMap::parse(&data, Some((terrain.size_x, terrain.size_z, terrain.height_range))) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("map parse failed: {e}");
            return 1;
        }
    };
    println!(
        "  scenery: {} verts, {} tris, {} runs, {} materials",
        map.scenery.vertices.len(),
        map.scenery.indices.len() / 3,
        map.scenery.submeshes.len(),
        map.materials.len()
    );

    // Ground check: scenery vertices nearest the terrain should sit on it.
    let mut diffs: Vec<f32> = map
        .scenery
        .vertices
        .iter()
        .step_by(97)
        .map(|v| v.position.y - terrain.height_at(v.position.x, v.position.z))
        .collect();
    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if !diffs.is_empty() {
        println!(
            "  scenery height above terrain: p5 {:.2} m, median {:.2} m",
            diffs[diffs.len() / 20],
            diffs[diffs.len() / 2]
        );
    }

    if let Some(t) = &map.terrain {
        for (i, l) in t.layers.iter().enumerate() {
            println!(
                "  layer {i}: {:?} tiling {:?} mask {:?} param {}",
                l.variants.iter().map(|v| v.diffuse.name.as_str()).collect::<Vec<_>>(),
                l.tiling,
                l.mask.as_ref().map(|m| (m.width, m.height, m.blob.format)),
                l.surface_param
            );
        }
        if let Some(dir) = out {
            std::fs::create_dir_all(&dir).ok();
            for (i, l) in t.layers.iter().enumerate() {
                if let Ok(img) = l.variants[0].diffuse.decode_frame(&data, 0) {
                    save_png(&img, &dir.join(format!("layer{i}_{}.png", l.variants[0].diffuse.name)));
                }
                if let Some(m) = &l.mask {
                    if let Ok(img) = m.blob.decode(&data, m.width, m.height) {
                        save_png(&img, &dir.join(format!("layer{i}_mask.png")));
                    }
                }
            }
            let (w, d) = (terrain.width, terrain.depth);
            let mut hm = image::GrayImage::new(w, d);
            for j in 0..d {
                for i in 0..w {
                    let h = terrain.heights[(j * w + i) as usize];
                    hm.put_pixel(i, j, image::Luma([((h as i32 + 32768) >> 8) as u8]));
                }
            }
            hm.save(dir.join("heightmap.png")).ok();
            println!("  wrote layers, masks and heightmap to {}", dir.display());
        }
    }
    0
}

fn report(t: f32, b: &mxbikes_physics::Motorcycle) {
    let (origin, rot) = b.chassis_transform();
    let (_, pitch, _) = rot.to_euler(glam::EulerRot::YXZ);
    println!(
        "  t {t:5.2}s  v {:6.1} km/h  rpm {:6.0} g{}  lean {:6.1}  pitch {:6.1}  steer {:6.2}  fork {:5.3}  shock {:5.3}  y {:5.3}  Fz f/r {:5.0}/{:5.0}  slip r {:5.2}{}",
        b.speed_kmh,
        b.powertrain.rpm(),
        b.powertrain.gear,
        b.lean_angle_deg,
        pitch.to_degrees(),
        b.steer_angle.to_degrees(),
        b.fork_compression,
        b.shock_compression(),
        origin.y,
        b.front_telemetry().load,
        b.rear_telemetry().load,
        b.rear_telemetry().slip_ratio,
        if b.crashed { "  CRASHED" } else { "" }
    );
}

fn sim(vfs: &MxBikesVfs, id: &str) -> i32 {
    use mxbikes_physics::{FlatGround, Motorcycle};
    let def = match assets::load_bike(vfs, id) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let set = assets::tyre_set_for(vfs, &def.tyres_type).unwrap_or_else(|| "p_mx".into());
    let Some(tyres) = assets::load_tyres(vfs, &set) else {
        eprintln!("tyre set {set} not loadable");
        return 1;
    };
    let ground = FlatGround;
    let dt = 1.0 / 60.0;
    // Scenarios choose gears explicitly, so the automatic gearbox assist is off.
    let new_bike = || {
        let mut b = Motorcycle::new(def.clone(), tyres.clone(), Vec3::ZERO, 0.0);
        b.powertrain.auto_shift = false;
        b
    };

    let mut b = new_bike();
    println!("{} on {set}: mass {:.1} kg, linkage solved: {}", def.name, b.total_mass(), b.is_linkage_solved());
    println!("settle (rider aboard, no input):");
    for i in 0..=180 {
        b.step(dt, &ground);
        if i % 60 == 0 {
            report(i as f32 * dt, &b);
        }
    }
    let settled = b.clone();

    println!("launch: 1st gear, full throttle, shift at shift rpm:");
    let mut b = settled.clone();
    b.powertrain.gear = 1;
    b.input.throttle = 1.0;
    for i in 1..=600 {
        if b.powertrain.rpm() > def.engine.shift_rpm && b.powertrain.gear < b.powertrain.gear_count() {
            b.input.shift_up = true;
        }
        b.step(dt, &ground);
        if i % 30 == 0 {
            report(i as f32 * dt, &b);
        }
        if !b.com.is_finite() {
            println!("  diverged");
            return 1;
        }
    }

    for v_kmh in [15.0f32, 30.0, 50.0] {
        println!("riderless self-stability at {v_kmh} km/h, roll-rate kick 0.3 rad/s:");
        let mut r = settled.clone();
        r.rider_steering = false;
        r.set_speed(v_kmh / 3.6);
        let fwd = r.rotation * glam::Vec3::NEG_Z;
        r.angular_velocity += fwd * 0.3;
        for i in 1..=180 {
            r.step(dt, &ground);
            if i % 30 == 0 {
                report(i as f32 * dt, &r);
            }
        }
    }

    for v_kmh in [30.0f32, 60.0, 110.0] {
        println!("steer step at {v_kmh} km/h (input 0.5 right, then -0.5 at 3s, release at 6s):");
        let mut c = settled.clone();
        c.steer_gain_scale = std::env::var("MXB_KSCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        c.powertrain.gear = 3;
        c.set_speed(v_kmh / 3.6);
        c.input.throttle = 0.3;
        for i in 1..=480 {
            c.input.steer = if i < 180 { 0.5 } else if i < 360 { -0.5 } else { 0.0 };
            c.step(dt, &ground);
            if i % 30 == 0 {
                report(i as f32 * dt, &c);
            }
        }
    }

    println!("emergency stop (both brakes, clutch in):");
    let mut d = b.clone();
    d.input = Default::default();
    d.input.front_brake = 1.0;
    d.input.rear_brake = 1.0;
    d.input.clutch = 1.0;
    let start = d.com;
    let v0 = d.speed_kmh;
    for i in 1..=600 {
        d.step(dt, &ground);
        if i % 30 == 0 {
            report(i as f32 * dt, &d);
        }
        if d.speed_kmh.abs() < 1.0 {
            break;
        }
    }
    println!("  stopped from {v0:.1} km/h in {:.1} m", (d.com - start).length());
    0
}

/// Rides straight out of the pits on a real track with part throttle and reports the run.
fn ride(vfs: &MxBikesVfs, track: &str) -> i32 {
    use mxbikes_physics::{Ground, Motorcycle, TerrainGround};
    let Some(stem) = assets::find_track(vfs, track) else { return 1 };
    let terrain = TrackTerrain::parse(&vfs.read(&format!("{stem}.trh")).unwrap()).unwrap();
    let ground = TerrainGround::new(terrain.clone());
    let def = assets::load_bike(vfs, "250xf").unwrap();
    let tyres = assets::load_tyres(vfs, "p_mx").unwrap();
    let start = assets::load_track_definition(vfs, &stem).and_then(|d| d.start).unwrap();
    let (x, z) = start.slot(0);
    let (x, z) = (x, -z);
    let heading = -start.angle_deg.to_radians();
    let pos = Vec3::new(x, ground.height(x, z), z);
    let mut b = Motorcycle::new(def, tyres, pos, heading);
    b.place_on(pos, heading);
    b.powertrain.auto_shift = false;
    let dt = 1.0 / 60.0;
    for i in 0..(60 * 20) {
        let t = i as f32 * dt;
        if i == 120 {
            b.input.shift_up = true;
        }
        if t > 2.0 {
            b.input.throttle = 0.6;
        }
        b.step(dt, &ground);
        if i % 20 == 0 || b.crashed {
            let (o, _) = b.chassis_transform();
            print!("pos ({:6.1},{:6.1},{:6.1}) ground {:6.2} ", o.x, o.y, o.z, ground.height(o.x, o.z));
            report(t, &b);
        }
        if b.crashed {
            break;
        }
    }
    0
}
