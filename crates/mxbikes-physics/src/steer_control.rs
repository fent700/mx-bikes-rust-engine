//! Rider handlebar torque, following the structure of MX Bikes' own rider steering controller:
//! a lean-target PID with speed-scheduled gains (`spg`, `sdg`, `sig` from the bike `.cfg`),
//! a yaw-rate term (`KYaw`) against the steady-turn yaw rate, a target lean limited by available
//! grip and rate limited, and a fade-out as front wheel load disappears.
//!
//! Angles are handled in degrees as in the original and every gain product is scaled by pi/180.

use mxbikes_formats::bike_def::SteerDefinition;

const DEG: f32 = std::f32::consts::PI / 180.0;
const LEAN_RATE_SAMPLES: usize = 5;
const YAW_RATE_SAMPLES: usize = 40;
pub const MAX_TORQUE: f32 = 250.0;

#[derive(Debug, Clone)]
pub struct SteerController {
    lean_rates: [f32; LEAN_RATE_SAMPLES],
    yaw_rates: [f32; YAW_RATE_SAMPLES],
    lean_index: usize,
    yaw_index: usize,
    prev_lean: Option<f32>,
    prev_heading: Option<f32>,
    target_lean: f32,
    integral: f32,
    /// Last computed terms for telemetry (N·m).
    pub terms: [f32; 4],
}

impl Default for SteerController {
    fn default() -> Self {
        Self {
            lean_rates: [0.0; LEAN_RATE_SAMPLES],
            yaw_rates: [0.0; YAW_RATE_SAMPLES],
            lean_index: 0,
            yaw_index: 0,
            prev_lean: None,
            prev_heading: None,
            target_lean: 0.0,
            integral: 0.0,
            terms: [0.0; 4],
        }
    }
}

/// Per-step inputs, angles in degrees.
pub struct SteerInputs {
    pub dt: f32,
    /// Stick -1 (left) .. 1 (right).
    pub stick: f32,
    /// Forward speed (m/s).
    pub speed: f32,
    /// Lean angle, positive to the right (deg).
    pub lean: f32,
    /// Heading, positive turning right (deg).
    pub heading: f32,
    /// Friction coefficient available at the tyres.
    pub grip: f32,
    /// Lateral ground slope under the bike (rad).
    pub bank: f32,
    /// Front wheel normal load and the bike + rider weight (N).
    pub front_load: f32,
    pub weight: f32,
}

impl SteerController {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Lean angle a rider may ask for, as the game computes it: the radius of the steady turn the
    /// tyre grip (with ground bank) supports at this speed; a radius under 1 m allows 90 degrees.
    /// The result is scaled by speed / 6 below 6 m/s. There is no other clamp in the original.
    fn max_lean(i: &SteerInputs, right: bool) -> f32 {
        let mu = i.grip * 0.8666;
        let theta = if right { i.bank } else { -i.bank };
        let (s, c) = theta.sin_cos();
        let v2 = i.speed * i.speed;
        let radius = ((c - s * mu) * v2 / ((c * mu + s) * 9.81)).abs();
        let mut lean = if radius >= 1.0 { (v2 / (radius * 9.81)).atan() / DEG } else { 90.0 };
        if i.speed < 6.0 {
            lean *= i.speed / 6.0;
        }
        lean
    }

    /// Returns handlebar torque (N·m) acting to increase the steer angle (positive = bars right).
    pub fn torque(&mut self, def: &SteerDefinition, i: &SteerInputs) -> f32 {
        let dt = i.dt.max(1e-5);

        let lean_rate = self.prev_lean.map(|p| (i.lean - p) / dt).unwrap_or(0.0);
        self.prev_lean = Some(i.lean);
        self.lean_rates[self.lean_index] = lean_rate;
        self.lean_index = (self.lean_index + 1) % LEAN_RATE_SAMPLES;
        let lean_rate = self.lean_rates.iter().sum::<f32>() / LEAN_RATE_SAMPLES as f32;

        let yaw_rate = self
            .prev_heading
            .map(|p| {
                let mut d = i.heading - p;
                if d > 180.0 {
                    d -= 360.0;
                } else if d < -180.0 {
                    d += 360.0;
                }
                d / dt
            })
            .unwrap_or(0.0);
        self.prev_heading = Some(i.heading);
        self.yaw_rates[self.yaw_index] = yaw_rate;
        self.yaw_index = (self.yaw_index + 1) % YAW_RATE_SAMPLES;
        let yaw_rate = self.yaw_rates.iter().sum::<f32>() / YAW_RATE_SAMPLES as f32;

        // Target lean from the stick, rate limited (slower at high speed).
        let limit = Self::max_lean(i, i.stick >= 0.0);
        let wanted = i.stick.clamp(-1.0, 1.0) * limit;
        let speed_factor = if i.speed >= 20.0 {
            1.0 - ((i.speed - 20.0) * 0.015).atan() / std::f32::consts::FRAC_PI_2
        } else {
            1.0
        };
        let max_step = 180.0 * speed_factor * dt;
        self.target_lean += (wanted - self.target_lean).clamp(-max_step, max_step);

        let error = i.lean - self.target_lean;
        self.integral += error * dt;

        let steady_yaw = if i.speed >= 1.0 {
            i.speed * (i.lean * DEG).tan() * 9.81 / (i.speed * i.speed) / DEG
        } else {
            0.0
        };

        let v = i.speed;
        let p = (def.spg[0] + def.spg[1] * v) * error * DEG;
        let d = (def.sdg[0] + def.sdg[1] * v) * lean_rate * DEG;
        let ig = (def.sig[0] + def.sig[1] * v) * self.integral * DEG;
        let yaw = def.k_yaw * (yaw_rate - steady_yaw) * DEG;
        self.terms = [p, d, ig, yaw];

        // Fade out as the front wheel unloads (wheelies, jumps).
        let reference = 0.45 * i.weight * 0.5;
        let blend = if i.front_load >= reference {
            1.0
        } else if i.front_load > 0.0 {
            (i.front_load / reference * std::f32::consts::FRAC_PI_2).sin()
        } else {
            0.0
        };
        if blend < 0.5 {
            self.integral *= 0.99;
        }
        ((p + d + ig + yaw) * blend).clamp(-MAX_TORQUE, MAX_TORQUE)
    }
}
