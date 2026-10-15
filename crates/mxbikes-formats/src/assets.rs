//! Resolves stock content through the VFS using the game's own folder conventions.

use crate::bike_def::BikeDefinition;
use crate::piboso_tree::ConfigNode;
use crate::track_def::TrackDefinition;
use crate::tyre_def::TyreDefinition;
use mxbikes_vfs::{MxBikesVfs, VfsError};

pub fn read_cfg(vfs: &MxBikesVfs, path: &str) -> Option<ConfigNode> {
    let bytes = vfs.read(path).ok()?;
    ConfigNode::parse(&String::from_utf8_lossy(&bytes)).ok()
}

pub fn bike_ids(vfs: &MxBikesVfs) -> Vec<String> {
    let mut ids: Vec<String> = vfs
        .list_files("bikes/")
        .into_iter()
        .filter_map(|p| {
            let mut parts = p.split('/');
            let (_, id, file) = (parts.next()?, parts.next()?, parts.next()?);
            (parts.next().is_none() && file == format!("{id}.cfg")).then(|| id.to_string())
        })
        .collect();
    ids.sort();
    ids
}

pub fn load_bike(vfs: &MxBikesVfs, id: &str) -> Result<BikeDefinition, VfsError> {
    let base = format!("bikes/{id}/");
    let cfg = read_cfg(vfs, &format!("{base}{id}.cfg"))
        .ok_or_else(|| VfsError::FileNotFound(format!("{base}{id}.cfg")))?;
    let geom_file = cfg.get_str("geom").map(str::to_string).unwrap_or(format!("{id}.geom"));
    let geom = read_cfg(vfs, &format!("{base}{geom_file}"));
    let engn_file = cfg
        .get_child("engine")
        .and_then(|e| {
            let idx = e.get_i64("mappingsetting").unwrap_or(0);
            e.get_child(&format!("mapping{idx}"))
        })
        .and_then(|m| m.get_str("curves"))
        .map(str::to_string)
        .unwrap_or(format!("{id}.engn"));
    let engn = read_cfg(vfs, &format!("{base}{engn_file}"));
    let ini = read_cfg(vfs, &format!("{base}{id}.ini"));
    Ok(BikeDefinition::from_nodes(&cfg, geom.as_ref(), engn.as_ref(), ini.as_ref()))
}

/// Finds the tyre set folder whose `<id>.ini` declares `[data] type = <kind>`.
pub fn tyre_set_for(vfs: &MxBikesVfs, kind: &str) -> Option<String> {
    let mut sets: Vec<String> = vfs
        .list_files("tyres/")
        .into_iter()
        .filter(|p| p.ends_with(".ini") && p.matches('/').count() == 2)
        .collect();
    sets.sort();
    sets.into_iter().find_map(|ini| {
        let node = read_cfg(vfs, &ini)?;
        let t = node.get_child("data")?.get_str("type")?.to_ascii_lowercase();
        (t == kind.to_ascii_lowercase()).then(|| ini.split('/').nth(1).unwrap().to_string())
    })
}

/// Loads the default (first) compound for front and rear wheels of a tyre set.
pub fn load_tyres(vfs: &MxBikesVfs, set: &str) -> Option<(TyreDefinition, TyreDefinition)> {
    let cfg = read_cfg(vfs, &format!("tyres/{set}/{set}.cfg"))?;
    let load = |wheel: &str| -> Option<TyreDefinition> {
        let params = cfg.get_child(wheel)?.get_child("tyre0")?.get_str("params")?.to_string();
        let node = read_cfg(vfs, &format!("tyres/{set}/{params}"))?;
        Some(TyreDefinition::from_node(&node))
    };
    Some((load("wheel0")?, load("wheel1")?))
}

/// Track folders contain `<name>.ini` + `<name>.trh`; layouts may live in sub folders.
pub fn track_paths(vfs: &MxBikesVfs) -> Vec<String> {
    let mut out: Vec<String> = vfs
        .list_files("tracks/")
        .into_iter()
        .filter_map(|p| p.strip_suffix(".trh").map(str::to_string))
        .collect();
    out.sort();
    out
}

/// Resolves a track by short folder name (e.g. `club`) to its file stem (`tracks/motocross/club/club`).
pub fn find_track(vfs: &MxBikesVfs, name: &str) -> Option<String> {
    let name = name.to_ascii_lowercase();
    if name == "menu" || name == "bg" {
        return Some("misc/bg/bg".to_string());
    }
    track_paths(vfs)
        .into_iter()
        .find(|p| p.rsplit('/').next() == Some(name.as_str()))
}

/// The scenery `.map` lives with the main layout; alternative layouts reuse the parent's.
pub fn track_map_path(vfs: &MxBikesVfs, stem: &str) -> Option<String> {
    let direct = format!("{stem}.map");
    if vfs.exists(&direct) {
        return Some(direct);
    }
    let folder = stem.rsplit_once('/')?.0;
    let parent = folder.rsplit_once('/')?.0;
    let parent_name = parent.rsplit('/').next()?;
    let candidate = format!("{parent}/{parent_name}.map");
    vfs.exists(&candidate).then_some(candidate)
}

pub fn load_track_definition(vfs: &MxBikesVfs, stem: &str) -> Option<TrackDefinition> {
    let ini = read_cfg(vfs, &format!("{stem}.ini"))?;
    let rdf = read_cfg(vfs, &format!("{stem}.rdf"));
    Some(TrackDefinition::from_nodes(&ini, rdf.as_ref()))
}

/// Resolves a file referenced relative to a folder, following PiBoSo's `*name` convention
/// (prefer the local folder, then the shared `misc/` fallback).
pub fn resolve_relative(vfs: &MxBikesVfs, folder: &str, file: &str) -> Option<String> {
    let file = file.trim_start_matches('*');
    let local = format!("{folder}/{file}");
    if vfs.exists(&local) {
        return Some(local);
    }
    let stem = file.trim_end_matches(".edf");
    [format!("misc/skies/{file}"), format!("misc/{file}"), format!("misc/skies/{stem}.edf")]
        .into_iter()
        .find(|p| vfs.exists(p))
}
