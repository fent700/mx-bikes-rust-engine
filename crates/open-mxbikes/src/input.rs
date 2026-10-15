//! Rider controls with the game's default bindings and filtering.
//!
//! Keyboard: the control table the game registers (`FUN_140022cd0`, table at 0x140395f40):
//! DirectInput scan codes per control, the control kind (1 single axis, 2 two-key axis,
//! 3 trigger on press, 4 held) and the press / release filter values f, which become key ramp
//! rates of 10 - 7 f per second (op 0x300; ramping in `FUN_14028b7f0`).
//!
//! Gamepad: the `controllers/xinput.cfg` profile of the game (axis / slider / button / pov per
//! control with deadzone and linearity), applied as `FUN_14028b7f0` does: deadzone rescale
//! (v - dz) / (1 - dz), then |v|^(1 + linearity).
//!
//! INFERRED: DirectInput Y axes grow downwards, so `sign = 1` on a stick's Y axis is the stick
//! pushed down; the sit control (a trigger in the table) toggles standing; xinput axis and button
//! numbers are mapped to Bevy's gamepad layout by the names the profile gives them.

use bevy::input::gamepad::{Gamepad, GamepadAxis, GamepadButton};
use bevy::prelude::*;
use mxbikes_formats::ConfigNode;
use mxbikes_physics::RiderInput;
use std::collections::HashMap;

/// The game's control kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Axis,
    TwoKeyAxis,
    Trigger,
    Hold,
}

struct KeyControl {
    name: &'static str,
    kind: Kind,
    smooth: bool,
    press: f32,
    release: f32,
    keys: [u32; 2],
}

/// The keyboard defaults of the game's control table.
const KEYBOARD: &[KeyControl] = &[
    KeyControl { name: "CTRL_LEAN", kind: Kind::TwoKeyAxis, smooth: true, press: 0.1, release: 0.1, keys: [0xcb, 0xcd] },
    KeyControl { name: "CTRL_THROTTLE", kind: Kind::Axis, smooth: true, press: 0.3, release: 0.1, keys: [0xc8, 0] },
    KeyControl { name: "CTRL_BRAKE", kind: Kind::Axis, smooth: true, press: 0.3, release: 0.1, keys: [0xd0, 0] },
    KeyControl { name: "CTRL_REARBRAKE", kind: Kind::Axis, smooth: true, press: 0.3, release: 0.1, keys: [0x38, 0] },
    KeyControl { name: "CTRL_CLUTCH", kind: Kind::Axis, smooth: true, press: 0.5, release: 0.05, keys: [0x2e, 0] },
    KeyControl { name: "CTRL_SHIFTUP", kind: Kind::Trigger, smooth: false, press: 0.0, release: 0.0, keys: [0x10, 0] },
    KeyControl { name: "CTRL_SHIFTDOWN", kind: Kind::Trigger, smooth: false, press: 0.0, release: 0.0, keys: [0x2c, 0] },
    KeyControl { name: "CTRL_FBLEAN", kind: Kind::TwoKeyAxis, smooth: true, press: 0.8, release: 0.8, keys: [0x1f, 0x11] },
    KeyControl { name: "CTRL_LRLEAN", kind: Kind::TwoKeyAxis, smooth: true, press: 0.8, release: 0.8, keys: [0x1e, 0x20] },
    KeyControl { name: "CTRL_SIT", kind: Kind::Trigger, smooth: false, press: 0.0, release: 0.0, keys: [0x12, 0] },
    KeyControl { name: "CTRL_CHANGEVIEW", kind: Kind::Trigger, smooth: false, press: 0.0, release: 0.0, keys: [0x2f, 0] },
    KeyControl { name: "CTRL_LOOKBACK", kind: Kind::Hold, smooth: false, press: 0.0, release: 0.0, keys: [0x1d, 0] },
    KeyControl { name: "CTRL_RESET", kind: Kind::Hold, smooth: false, press: 0.0, release: 0.0, keys: [0x13, 0] },
    KeyControl { name: "CTRL_STARTER", kind: Kind::Hold, smooth: false, press: 0.0, release: 0.0, keys: [0x25, 0] },
    KeyControl { name: "CTRL_DAB", kind: Kind::Hold, smooth: false, press: 0.0, release: 0.0, keys: [0x21, 0] },
    KeyControl { name: "CTRL_HEADING", kind: Kind::TwoKeyAxis, smooth: false, press: 0.0, release: 0.0, keys: [0x33, 0x34] },
    KeyControl { name: "CTRL_TEAROFF", kind: Kind::Trigger, smooth: false, press: 0.0, release: 0.0, keys: [0x2d, 0] },
    KeyControl { name: "CTRL_RAISEARM", kind: Kind::Hold, smooth: false, press: 0.0, release: 0.0, keys: [0x32, 0] },
    KeyControl { name: "CTRL_COMPLAIN", kind: Kind::Trigger, smooth: false, press: 0.0, release: 0.0, keys: [0x31, 0] },
    KeyControl { name: "CTRL_WINGESTURE", kind: Kind::Trigger, smooth: false, press: 0.0, release: 0.0, keys: [0x30, 0] },
    KeyControl { name: "CTRL_ANGRYGESTURE", kind: Kind::Trigger, smooth: false, press: 0.0, release: 0.0, keys: [0x23, 0] },
    KeyControl { name: "CTRL_CHAT", kind: Kind::Trigger, smooth: false, press: 0.0, release: 0.0, keys: [0x14, 0] },
    KeyControl { name: "CTRL_LOWSPEED", kind: Kind::TwoKeyAxis, smooth: false, press: 0.0, release: 0.0, keys: [0x26, 0x18] },
    KeyControl { name: "CTRL_TRACKMARKER", kind: Kind::Hold, smooth: false, press: 0.0, release: 0.0, keys: [0x16, 0] },
    KeyControl { name: "CTRL_TRICK", kind: Kind::Hold, smooth: false, press: 0.0, release: 0.0, keys: [0x22, 0] },
];

/// Settings item of each control (`FUN_1400cc440`) and the string ids the capture dialog shows
/// for its first and second key (`FUN_1400cb6b0`, `FUN_1400d8550`).
pub const CONTROL_ITEMS: &[(&str, &str, &str, &str)] = &[
    ("id_lean", "CTRL_LEAN", "LeanLeft", "LeanRight"),
    ("id_throttle", "CTRL_THROTTLE", "Throttle", ""),
    ("id_brake", "CTRL_BRAKE", "Brake", ""),
    ("id_rearbrake", "CTRL_REARBRAKE", "RearBrake", ""),
    ("id_clutch", "CTRL_CLUTCH", "Clutch", ""),
    ("id_shiftup", "CTRL_SHIFTUP", "Shiftup", ""),
    ("id_shiftdown", "CTRL_SHIFTDOWN", "Shiftdown", ""),
    ("id_fblean", "CTRL_FBLEAN", "LeanRiderBackward", "LeanRiderForward"),
    ("id_lrlean", "CTRL_LRLEAN", "LeanRiderLeft", "LeanRiderRight"),
    ("id_sit", "CTRL_SIT", "Sit", ""),
    ("id_changeview", "CTRL_CHANGEVIEW", "changeview", ""),
    ("id_lookback", "CTRL_LOOKBACK", "lookback", ""),
    ("id_heading", "CTRL_HEADING", "lookleft", "lookright"),
    ("id_tearoff", "CTRL_TEAROFF", "tearoff", ""),
    ("id_raisearm", "CTRL_RAISEARM", "RaiseArm", ""),
    ("id_complaint", "CTRL_COMPLAIN", "Complaint", ""),
    ("id_wingesture", "CTRL_WINGESTURE", "WinGesture", ""),
    ("id_angrygesture", "CTRL_ANGRYGESTURE", "AngryGesture", ""),
    ("id_chat", "CTRL_CHAT", "chat", ""),
    ("id_lowspeed", "CTRL_LOWSPEED", "LowSpeedBack", "LowSpeedForw"),
    ("id_reset", "CTRL_RESET", "reset", ""),
    ("id_starter", "CTRL_STARTER", "starter", ""),
    ("id_dab", "CTRL_DAB", "Dab", ""),
    ("id_trackmarker", "CTRL_TRACKMARKER", "trackmarker", ""),
    ("id_trick", "CTRL_TRICK", "trick", ""),
];

/// Key names of the game's table (0x140395b00); other keys print their character, or "Key %d".
const KEY_NAMES: &[(u32, &str)] = &[
    (0x01, "Escape"), (0x0e, "Backspace"), (0x0f, "Tab"), (0x1c, "Enter"), (0x1d, "LCtrl"), (0x9d, "RCtrl"),
    (0x2a, "LShift"), (0x36, "RShift"), (0x38, "LAlt"), (0xb8, "RAlt"), (0x39, "Space"), (0x3a, "Caps"),
    (0x3b, "F1"), (0x3c, "F2"), (0x3d, "F3"), (0x3e, "F4"), (0x3f, "F5"), (0x40, "F6"), (0x41, "F7"),
    (0x42, "F8"), (0x43, "F9"), (0x44, "F10"), (0x57, "F11"), (0x58, "F12"), (0xd2, "Ins"), (0xc7, "Home"),
    (0xc9, "PgUp"), (0xd3, "Del"), (0xd1, "PgDn"), (0xcf, "End"), (0xc8, "Up"), (0xcb, "Left"), (0xcd, "Right"),
    (0xd0, "Down"), (0x47, "NP7"), (0x48, "NP8"), (0x49, "NP9"), (0x4a, "NP-"), (0x4b, "NP4"), (0x4c, "NP5"),
    (0x4d, "NP6"), (0x4e, "NP+"), (0x4f, "NP1"), (0x50, "NP2"), (0x51, "NP3"), (0x52, "NP0"), (0x53, "NP."),
    (0x9c, "NPEnter"), (0xb5, "NP/"), (0x37, "NP*"),
];

/// Text of one key as the control buttons show it (`FUN_1400cad40`).
pub fn key_text(code: u32) -> String {
    if code == 0 {
        return "Unassigned".into();
    }
    if let Some((_, n)) = KEY_NAMES.iter().find(|(c, _)| *c == code) {
        return n.to_string();
    }
    let ch = match dik(code) {
        Some(KeyCode::Comma) => Some(','),
        Some(KeyCode::Period) => Some('.'),
        Some(k) => {
            let name = format!("{k:?}");
            name.strip_prefix("Key").or_else(|| name.strip_prefix("Digit")).and_then(|c| c.chars().next())
        }
        None => None,
    };
    match ch {
        Some(c) => c.to_string(),
        None => format!("Key {code}"),
    }
}

/// The keyboard bindings: the game's defaults, overridden by the profile `controls` section.
#[derive(Resource, Debug, Clone)]
pub struct Bindings {
    pub keys: HashMap<&'static str, [u32; 2]>,
}

impl Bindings {
    pub fn from_profile(profile: &crate::profile::Profile) -> Self {
        let mut keys = HashMap::new();
        for c in KEYBOARD {
            let saved = profile.str("controls", c.name);
            let parsed: Vec<u32> = saved.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            let k = if parsed.len() == 2 { [parsed[0], parsed[1]] } else { c.keys };
            keys.insert(c.name, k);
        }
        Self { keys }
    }

    pub fn save(&self, profile: &mut crate::profile::Profile) {
        for (name, k) in &self.keys {
            profile.set("controls", name, format!("{},{}", k[0], k[1]));
        }
    }

    /// Button text of a control item ("Left Right" for a two-key axis).
    pub fn item_text(&self, item: &str) -> Option<String> {
        let (_, ctrl, _, second) = CONTROL_ITEMS.iter().find(|c| c.0 == item)?;
        let k = self.keys.get(ctrl)?;
        Some(if second.is_empty() { key_text(k[0]) } else { format!("{} {}", key_text(k[0]), key_text(k[1])) })
    }

    pub fn is_two_key(item: &str) -> bool {
        CONTROL_ITEMS.iter().any(|c| c.0 == item && !c.3.is_empty())
    }

    pub fn assign(&mut self, item: &str, step: usize, code: u32) {
        if let Some((_, ctrl, _, _)) = CONTROL_ITEMS.iter().find(|c| c.0 == item) {
            if let Some(k) = self.keys.get_mut(ctrl) {
                k[step.min(1)] = code;
            }
        }
    }
}

/// DirectInput scan code of a key (inverse of [`dik`]).
pub fn key_dik(key: KeyCode) -> Option<u32> {
    (1..0x100).find(|&c| dik(c) == Some(key))
}

/// DirectInput scan code to key.
fn dik(code: u32) -> Option<KeyCode> {
    Some(match code {
        0x01 => KeyCode::Escape,
        0x02 => KeyCode::Digit1,
        0x03 => KeyCode::Digit2,
        0x04 => KeyCode::Digit3,
        0x05 => KeyCode::Digit4,
        0x06 => KeyCode::Digit5,
        0x07 => KeyCode::Digit6,
        0x08 => KeyCode::Digit7,
        0x09 => KeyCode::Digit8,
        0x0a => KeyCode::Digit9,
        0x0b => KeyCode::Digit0,
        0x0f => KeyCode::Tab,
        0x10 => KeyCode::KeyQ,
        0x11 => KeyCode::KeyW,
        0x12 => KeyCode::KeyE,
        0x13 => KeyCode::KeyR,
        0x14 => KeyCode::KeyT,
        0x15 => KeyCode::KeyY,
        0x16 => KeyCode::KeyU,
        0x17 => KeyCode::KeyI,
        0x18 => KeyCode::KeyO,
        0x19 => KeyCode::KeyP,
        0x1c => KeyCode::Enter,
        0x1d => KeyCode::ControlLeft,
        0x1e => KeyCode::KeyA,
        0x1f => KeyCode::KeyS,
        0x20 => KeyCode::KeyD,
        0x21 => KeyCode::KeyF,
        0x22 => KeyCode::KeyG,
        0x23 => KeyCode::KeyH,
        0x24 => KeyCode::KeyJ,
        0x25 => KeyCode::KeyK,
        0x26 => KeyCode::KeyL,
        0x2a => KeyCode::ShiftLeft,
        0x2c => KeyCode::KeyZ,
        0x2d => KeyCode::KeyX,
        0x2e => KeyCode::KeyC,
        0x2f => KeyCode::KeyV,
        0x30 => KeyCode::KeyB,
        0x31 => KeyCode::KeyN,
        0x32 => KeyCode::KeyM,
        0x33 => KeyCode::Comma,
        0x34 => KeyCode::Period,
        0x36 => KeyCode::ShiftRight,
        0x38 => KeyCode::AltLeft,
        0x39 => KeyCode::Space,
        0x3b => KeyCode::F1,
        0x42 => KeyCode::F8,
        0x9d => KeyCode::ControlRight,
        0xb8 => KeyCode::AltRight,
        0xc8 => KeyCode::ArrowUp,
        0xc9 => KeyCode::PageUp,
        0xcb => KeyCode::ArrowLeft,
        0xcd => KeyCode::ArrowRight,
        0xd0 => KeyCode::ArrowDown,
        0xd1 => KeyCode::PageDown,
        0xd2 => KeyCode::Insert,
        0xd3 => KeyCode::Delete,
        _ => return None,
    })
}

/// One control of the game's xinput profile.
#[derive(Debug, Clone)]
struct PadControl {
    kind: String,
    num: u32,
    sign: i32,
    direction: String,
    deadzone: f32,
    linearity: f32,
}

/// The controller profile read from `controllers/xinput.cfg`.
#[derive(Resource, Debug, Clone, Default)]
pub struct PadProfile {
    controls: HashMap<String, PadControl>,
}

impl PadProfile {
    pub fn from_cfg(cfg: &ConfigNode) -> Self {
        let mut controls = HashMap::new();
        for c in cfg.children.iter().filter(|c| c.name.to_ascii_lowercase().starts_with("control")) {
            let (Some(name), Some(input)) = (c.get_str("name"), c.get_child("input")) else { continue };
            controls.insert(
                name.to_ascii_uppercase(),
                PadControl {
                    kind: input.get_str("type").unwrap_or("").to_ascii_lowercase(),
                    num: input.get_i64("num").unwrap_or(0) as u32,
                    sign: input.get_i64("sign").unwrap_or(1) as i32,
                    direction: input.get_str("direction").unwrap_or("").to_ascii_uppercase(),
                    deadzone: c.get_f32("deadzone").unwrap_or(0.0),
                    linearity: c.get_f32("linearity").unwrap_or(0.0),
                },
            );
        }
        Self { controls }
    }
}

#[derive(Resource, Default)]
pub struct ControlState {
    pub rider: RiderInput,
    pub reset: bool,
    pub toggle_camera: bool,
    /// Filtered keyboard values per control.
    filtered: HashMap<&'static str, f32>,
    /// Latched stand request toggled by the sit control.
    standing: bool,
    /// Analog inputs are only trusted after they have been seen at rest once, so devices that
    /// report a stuck or inverted axis (pedal sets, wheels) cannot hold the throttle open.
    armed: std::collections::HashSet<(Entity, u8)>,
}

fn armed_value(armed: &mut std::collections::HashSet<(Entity, u8)>, pad: Entity, id: u8, value: f32, rest: f32) -> f32 {
    if (value - rest).abs() < 0.05 {
        armed.insert((pad, id));
    }
    if armed.contains(&(pad, id)) {
        value
    } else {
        rest
    }
}

/// Deadzone and linearity of `FUN_14028b7f0`.
fn shape(v: f32, deadzone: f32, linearity: f32) -> f32 {
    let a = v.abs();
    if a <= deadzone {
        return 0.0;
    }
    let a = (a - deadzone) / (1.0 - deadzone).max(1e-3);
    a.powf(linearity + 1.0).copysign(v)
}

/// Value of an xinput control on a pad (`None` when the profile has no such control).
fn pad_value(pad: &Gamepad, c: &PadControl) -> f32 {
    let axis = |n: u32| match n {
        1 => pad.get(GamepadAxis::LeftStickX).unwrap_or(0.0),
        // DirectInput Y grows downwards.
        2 => -pad.get(GamepadAxis::LeftStickY).unwrap_or(0.0),
        3 => pad.get(GamepadAxis::RightStickX).unwrap_or(0.0),
        4 => -pad.get(GamepadAxis::RightStickY).unwrap_or(0.0),
        _ => 0.0,
    };
    let button = |n: u32| {
        let b = match n {
            1 => GamepadButton::Start,
            2 => GamepadButton::Select,
            3 => GamepadButton::LeftThumb,
            4 => GamepadButton::RightThumb,
            5 => GamepadButton::LeftTrigger,
            6 => GamepadButton::RightTrigger,
            7 => GamepadButton::South,
            8 => GamepadButton::East,
            9 => GamepadButton::West,
            10 => GamepadButton::North,
            _ => return 0.0,
        };
        if pad.pressed(b) {
            1.0
        } else {
            0.0
        }
    };
    let raw = match c.kind.as_str() {
        "axis" if c.num != 1 && c.num != 3 => (axis(c.num) * c.sign as f32).max(0.0),
        "axis" => axis(c.num) * c.sign as f32,
        "slider" => match c.num {
            1 => pad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0),
            2 => pad.get(GamepadButton::RightTrigger2).unwrap_or(0.0),
            _ => 0.0,
        },
        "button" => button(c.num),
        "pov" => {
            let b = match c.direction.as_str() {
                "N" => GamepadButton::DPadUp,
                "S" => GamepadButton::DPadDown,
                "E" => GamepadButton::DPadRight,
                "W" => GamepadButton::DPadLeft,
                _ => return 0.0,
            };
            if pad.pressed(b) {
                1.0
            } else {
                0.0
            }
        }
        _ => 0.0,
    };
    shape(raw, c.deadzone, c.linearity)
}

pub fn read_controls(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    pad_profile: Option<Res<PadProfile>>,
    bindings: Option<Res<Bindings>>,
    mut state: ResMut<ControlState>,
) {
    let dt = time.delta_secs();
    let bound = |c: &KeyControl| bindings.as_ref().and_then(|b| b.keys.get(c.name).copied()).unwrap_or(c.keys);
    let held = |code: u32| dik(code).is_some_and(|k| keys.pressed(k));
    let pressed_now = |code: u32| dik(code).is_some_and(|k| keys.just_pressed(k));
    let mut values: HashMap<&'static str, f32> = HashMap::new();
    let mut triggers: HashMap<&'static str, bool> = HashMap::new();
    for c in KEYBOARD {
        let keys = bound(c);
        match c.kind {
            Kind::Axis | Kind::TwoKeyAxis => {
                let target = if c.kind == Kind::Axis {
                    if held(keys[0]) { 1.0 } else { 0.0 }
                } else {
                    (held(keys[1]) as i32 - held(keys[0]) as i32) as f32
                };
                let v = if c.smooth {
                    let current = state.filtered.get(c.name).copied().unwrap_or(0.0);
                    // Moving away from rest uses the press rate, back towards rest the release
                    // rate; a two-key axis crossing zero uses the press rate.
                    let rate = if target.abs() > current.abs() || target * current < 0.0 {
                        10.0 - 7.0 * c.press
                    } else {
                        10.0 - 7.0 * c.release
                    };
                    let next = current + (target - current).clamp(-rate * dt, rate * dt);
                    state.filtered.insert(c.name, next);
                    next
                } else {
                    target
                };
                values.insert(c.name, v);
            }
            Kind::Trigger => {
                triggers.insert(c.name, pressed_now(keys[0]));
            }
            Kind::Hold => {
                values.insert(c.name, if held(keys[0]) { 1.0 } else { 0.0 });
                triggers.insert(c.name, pressed_now(keys[0]));
            }
        }
    }

    if let Some(profile) = pad_profile.as_ref() {
        for (entity, pad) in &gamepads {
            for (name, c) in &profile.controls {
                let Some(key) = KEYBOARD.iter().map(|k| k.name).find(|k| k == name) else { continue };
                let id = KEYBOARD.iter().position(|k| k.name == key).unwrap_or(0) as u8;
                let v = armed_value(&mut state.armed, entity, id, pad_value(pad, c), 0.0);
                match c.kind.as_str() {
                    "button" | "pov" => {
                        if v > 0.5 {
                            values.insert(key, 1.0);
                        }
                        let just = match c.num {
                            _ if c.kind == "pov" => false,
                            n => {
                                let b = [
                                    GamepadButton::Start,
                                    GamepadButton::Select,
                                    GamepadButton::LeftThumb,
                                    GamepadButton::RightThumb,
                                    GamepadButton::LeftTrigger,
                                    GamepadButton::RightTrigger,
                                    GamepadButton::South,
                                    GamepadButton::East,
                                    GamepadButton::West,
                                    GamepadButton::North,
                                ];
                                b.get(n.wrapping_sub(1) as usize).is_some_and(|b| pad.just_pressed(*b))
                            }
                        };
                        if just {
                            triggers.insert(key, true);
                        }
                    }
                    _ => {
                        if v.abs() > values.get(key).copied().unwrap_or(0.0).abs() {
                            values.insert(key, v);
                        }
                    }
                }
            }
        }
    }

    let v = |n: &str| values.get(n).copied().unwrap_or(0.0);
    let t = |n: &str| triggers.get(n).copied().unwrap_or(false);
    if t("CTRL_SIT") {
        state.standing = !state.standing;
    }
    state.reset = t("CTRL_RESET");
    state.toggle_camera = t("CTRL_CHANGEVIEW");
    state.rider = RiderInput {
        steer: v("CTRL_LEAN"),
        throttle: v("CTRL_THROTTLE"),
        front_brake: v("CTRL_BRAKE"),
        rear_brake: v("CTRL_REARBRAKE"),
        clutch: v("CTRL_CLUTCH"),
        lean_forward: v("CTRL_FBLEAN"),
        lean_lateral: v("CTRL_LRLEAN"),
        stand: state.standing,
        shift_up: t("CTRL_SHIFTUP"),
        shift_down: t("CTRL_SHIFTDOWN"),
        starter: v("CTRL_STARTER") > 0.5,
    };
    if state.reset {
        state.filtered.clear();
        state.rider.steer = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadzone_and_linearity() {
        assert_eq!(shape(0.1, 0.2, 0.0), 0.0);
        assert!((shape(0.6, 0.2, 0.0) - 0.5).abs() < 1e-6);
        assert!((shape(-0.6, 0.2, 1.0) + 0.25).abs() < 1e-6);
    }

    #[test]
    fn xinput_profile_parses() {
        let cfg = ConfigNode::parse("control0\n{\n\tname = CTRL_LEAN\n\tinput\n\t{\n\t\ttype = axis\n\t\tnum = 1\n\t\tsign = 1\n\t}\n\tlinearity = 0\n\tdeadzone = 0.2\n}\n").unwrap();
        let p = PadProfile::from_cfg(&cfg);
        let c = &p.controls["CTRL_LEAN"];
        assert_eq!((c.kind.as_str(), c.num, c.deadzone), ("axis", 1, 0.2));
    }
}
