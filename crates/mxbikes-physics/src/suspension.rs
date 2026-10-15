//! Front fork and rear linkage suspension from the stock `.cfg` setup and `.geom` hard points.

use glam::Vec2;
use mxbikes_formats::bike_def::{BikeGeometry, FrontSuspensionDefinition, RearSuspensionDefinition};

const ATM_KPA: f32 = 101.325;
const AIR_GAMMA: f32 = 1.4;
const LEGS: f32 = 2.0;

/// Telescopic fork: per-leg coil spring with preload, air column compressed by travel, three
/// stage bump/rebound damping. `compression` is metres of slider travel (0 = topped out).
#[derive(Debug, Clone)]
pub struct Fork {
    pub def: FrontSuspensionDefinition,
    pub travel: f32,
}

impl Fork {
    pub fn new(def: FrontSuspensionDefinition, travel: f32) -> Self {
        Self { def, travel }
    }

    /// Air gap above the oil at full extension.
    fn air_gap(&self) -> f32 {
        (self.travel + self.def.air_length_base + self.def.oil_level).max(0.02)
    }

    /// Axial force pushing the fork apart (N), both legs.
    pub fn force(&self, compression: f32, speed: f32) -> f32 {
        let d = &self.def;
        let x = compression.clamp(0.0, self.travel);
        let spring = d.spring_rate * (x + d.preload);
        let gap = self.air_gap();
        let ratio = gap / (gap - x).max(gap * 0.08);
        let air = (d.air_pressure_kpa.max(ATM_KPA) * ratio.powf(AIR_GAMMA) - ATM_KPA) * 1000.0 * d.air_area;
        let damping = if speed >= 0.0 {
            d.bump.force(speed)
        } else {
            d.rebound.force(speed)
        };
        let mut f = LEGS * (spring + air.max(0.0) + damping);
        // Hydraulic bottoming cone over the last 15 mm, top-out spring below zero.
        let bottom = x - (self.travel - 0.015);
        if bottom > 0.0 {
            f += 4.0e6 * bottom * bottom + 3000.0 * speed.max(0.0);
        }
        if compression < 0.0 {
            f += 2.0e5 * compression;
        }
        f
    }
}

/// Rear shock with rising-rate linkage solved from the `.geom` side-view points.
///
/// All points are in the asset side view (y up, z forward). The swingarm rotates about the pivot
/// by `angle` (positive = wheel up). The rocker hangs from the swingarm at `rocker_swingarm`; a rod
/// of `rod_length` ties the rocker's far end to the chassis at `rod_chassis`; the shock runs from
/// `shock_chassis` to the rocker point `shock_rocker` (rocker frame: y along pivot->rod end).
#[derive(Debug, Clone)]
pub struct RearLinkage {
    pivot: Vec2,
    rocker_arm: Vec2,
    rocker_len: f32,
    rod_chassis: Vec2,
    rod_len: f32,
    shock_chassis: Vec2,
    shock_rocker: Vec2,
    ref_angle: f32,
    pub axle_arm: Vec2,
    pub shock_max: f32,
    pub shock_min: f32,
    /// Swingarm angle range (droop, bottom) in radians.
    pub angle_range: (f32, f32),
    /// Fallback linear ratio when the linkage cannot be solved.
    linear_ratio: Option<f32>,
}

#[inline]
fn yz(v: glam::Vec3) -> Vec2 {
    Vec2::new(v.y, v.z)
}

#[inline]
fn rot(p: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c)
}

impl RearLinkage {
    pub fn new(g: &BikeGeometry, rod_adjust: f32) -> Self {
        let mut l = Self {
            pivot: yz(g.swingarm_pivot),
            rocker_arm: yz(g.rocker_swingarm),
            rocker_len: g.rocker_length,
            rod_chassis: yz(g.rod_chassis),
            rod_len: g.rod_length + rod_adjust,
            shock_chassis: yz(g.shock_chassis),
            shock_rocker: yz(g.shock_rocker),
            ref_angle: g.rocker_ref_angle_deg.to_radians(),
            axle_arm: yz(g.rwheel),
            shock_max: g.shock_min_length + g.rear_length,
            shock_min: g.shock_min_length,
            angle_range: (-0.5, 0.5),
            linear_ratio: None,
        };
        l.calibrate();
        l
    }

    /// Shock eye-to-eye length at a swingarm angle, or None when the linkage locks.
    pub fn shock_length_raw(&self, angle: f32) -> Option<f32> {
        let a = self.pivot + rot(self.rocker_arm, angle);
        let c = self.rod_chassis;
        let d = (c - a).length();
        let (r1, r2) = (self.rocker_len, self.rod_len);
        if d > r1 + r2 || d < (r1 - r2).abs() || d < 1e-6 {
            return None;
        }
        let along = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
        let h = (r1 * r1 - along * along).max(0.0).sqrt();
        let e = (c - a) / d;
        let n = Vec2::new(-e.y, e.x);
        let mid = a + e * along;
        let candidates = [mid + n * h, mid - n * h];
        // Pick the rocker branch nearest the reference orientation.
        let angle_of = |p: Vec2| (p.x - a.x).atan2(p.y - a.y);
        let diff = |p: Vec2| {
            let x = angle_of(p) - self.ref_angle;
            x.sin().atan2(x.cos()).abs()
        };
        let b = if diff(candidates[0]) <= diff(candidates[1]) {
            candidates[0]
        } else {
            candidates[1]
        };
        let y_axis = (b - a).normalize_or_zero();
        let z_axis = Vec2::new(y_axis.y, -y_axis.x);
        let s = a + y_axis * self.shock_rocker.x + z_axis * self.shock_rocker.y;
        Some((s - self.shock_chassis).length())
    }

    fn calibrate(&mut self) {
        let samples: Vec<(f32, f32)> = (-60..=60)
            .filter_map(|i| {
                let a = i as f32 * 0.01;
                self.shock_length_raw(a).map(|s| (a, s))
            })
            .collect();
        let find = |target: f32| {
            samples
                .windows(2)
                .find(|w| (w[0].1 - target) * (w[1].1 - target) <= 0.0 && w[1].1 < w[0].1)
                .map(|w| w[0].0 + (w[1].0 - w[0].0) * (target - w[0].1) / (w[1].1 - w[0].1))
        };
        // Droop is limited either by the shock's full length or by the linkage's reach.
        let droop = find(self.shock_max).or_else(|| {
            samples
                .windows(2)
                .find(|w| w[1].1 < w[0].1)
                .map(|w| w[0].0)
        });
        let bottom = find(self.shock_min);
        let monotonic = match (droop, bottom) {
            (Some(d), Some(b)) => samples
                .windows(2)
                .filter(|w| w[0].0 >= d && w[1].0 <= b)
                .all(|w| w[1].1 < w[0].1),
            _ => false,
        };
        match (monotonic, droop, bottom) {
            (true, Some(droop), Some(bottom)) if bottom > droop => {
                self.angle_range = (droop, bottom);
            }
            _ => {
                // Linear fallback: a typical 310 mm wheel travel over the shock stroke.
                let arm = self.axle_arm.length().max(0.3);
                let wheel_travel = 0.31;
                let half = (wheel_travel / arm).asin() * 0.5;
                self.angle_range = (-half * 0.4, half * 1.6);
                self.linear_ratio = Some((self.shock_max - self.shock_min) / (self.angle_range.1 - self.angle_range.0));
            }
        }
    }

    pub fn shock_length(&self, angle: f32) -> f32 {
        if let Some(k) = self.linear_ratio {
            return self.shock_max - (angle - self.angle_range.0) * k;
        }
        let a = angle.clamp(self.angle_range.0 - 0.2, self.angle_range.1 + 0.2);
        self.shock_length_raw(a).unwrap_or(self.shock_min)
    }

    /// d(shock length)/d(angle), negative: the shock shortens as the wheel rises.
    pub fn shock_rate(&self, angle: f32) -> f32 {
        let h = 1e-3;
        (self.shock_length(angle + h) - self.shock_length(angle - h)) / (2.0 * h)
    }

    /// Axle position relative to the pivot (side view y,z) at an angle.
    pub fn axle(&self, angle: f32) -> Vec2 {
        rot(self.axle_arm, angle)
    }

    pub fn is_linkage_solved(&self) -> bool {
        self.linear_ratio.is_none()
    }
}

/// Rear shock force along its axis (pushing the eyes apart), from compression and speed.
#[derive(Debug, Clone)]
pub struct Shock {
    pub def: RearSuspensionDefinition,
}

impl Shock {
    /// `compression` = max length - current length; `speed` = d(compression)/dt.
    pub fn force(&self, compression: f32, stroke: f32, speed: f32) -> f32 {
        let d = &self.def;
        let x = compression.clamp(0.0, stroke);
        let mut f = d.spring_rate * (x + d.preload);
        f += if speed >= 0.0 {
            d.bump.force(speed)
        } else {
            d.rebound.force(speed)
        };
        let bs = &d.bump_stop;
        let engage = stroke - bs.length.max(0.001);
        if x > engage {
            let p = (x - engage) / bs.length.max(0.001);
            f += bs.spring_base * 1000.0 * p * (1.0 + bs.spring_rising_rate * 10.0 * p)
                + bs.damping_base * 1000.0 * speed.max(0.0) * (1.0 + bs.damping_rising_rate * 10.0 * p);
        }
        if compression > stroke {
            f += 5.0e6 * (compression - stroke);
        }
        if compression < 0.0 {
            f += 1.0e6 * compression;
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxbikes_formats::{BikeDefinition, ConfigNode};

    /// Stock 250xf.geom rear hard points.
    fn geometry() -> BikeGeometry {
        let geom = ConfigNode::parse(concat!(
            "chassis_rsusp_min = 0, 0.47, -0.17\nrear_length = 0.12\nshock_minlength = 0.375\n",
            "shock_chassis = 0, 0.785, -0.15\nrocker_swingarm = 0, -0.0457, -0.1511\nrocker_length = 0.045\n",
            "rod_chassis = 0, 0.3762, -0.2051\nrod_length = 0.145\nshock_rocker = 0, 0.0493, 0.0398\n",
            "ref_angle = -90\nrwheel_min = 0, -0.001, -0.5513\n"
        ))
        .unwrap();
        BikeDefinition::from_nodes(&ConfigNode::new(""), Some(&geom), None, None).geometry
    }

    #[test]
    fn linkage_solves_with_rising_rate() {
        let l = RearLinkage::new(&geometry(), 0.0);
        let samples: Vec<_> = (-30..=30).step_by(5).map(|d| (d, l.shock_length_raw((d as f32).to_radians()))).collect();
        assert!(l.is_linkage_solved(), "samples {samples:?} range {:?}", l.angle_range);
        let (lo, hi) = l.angle_range;
        let travel = l.axle(hi).x - l.axle(lo).x;
        assert!(travel > 0.25 && travel < 0.4, "wheel travel {travel}");
        // Rising rate: the shock moves faster per unit wheel travel near bottom-out.
        assert!(l.shock_rate(hi).abs() > l.shock_rate(lo).abs());
    }
}
