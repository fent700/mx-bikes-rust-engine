//! Rider handlebar torque, following the structure of MX Bikes' own rider steering controller:
//! a lean-target PID with speed-scheduled gains (`spg`, `sdg`, `sig` from the bike `.cfg`),
//! a yaw-rate term (`KYaw`) against the steady-turn yaw rate, a target lean limited by available
//! grip and rate limited, and a fade-out as front wheel load disappears.
//!
//! Angles are handled in degrees as in the original and every gain product is scaled by pi/180.

use mxbikes_formats::bike_def::SteerDefinition;

const DEG: f32 = std::f32::consts::PI / 180.0;

fn sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}
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
    /// Weight of the lean PID against the bar-centring PD, from the front wheel load: 1 at
    /// 0.45 * m g / 2 and above, sin(load / ref * pi / 2) below (FUN_14019d360).
    pub blend: f32,
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
            blend: 1.0,
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
    /// Lateral friction coefficient of the front tyre (the game reads the tyre's last mu_y).
    pub grip: f32,
    /// Profile aid `leanhelp` bits: 1 = rate-limited, load-blended lean target; 2 = target
    /// clamped to the grip-limited lean (else scaled by speed / 6 below 6 m/s).
    pub lean_help: u32,
    /// Profile aid `leanhelp_scale`: return-to-upright rate as a fraction of 180 deg/s.
    pub lean_help_scale: f32,
    /// Front tyre on a loose surface (game surface types 4, 5, 7, 9-12, 14): grip x 1.4 for
    /// the lean help limit.
    pub loose_surface: bool,
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
    fn max_lean(i: &SteerInputs, mu: f32, right: bool) -> f32 {
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

    /// Returns the lean PID handlebar torque (N·m) acting to increase the steer angle (positive =
    /// bars right), before the front-load blend in [`Self::blend`] and the final clamp.
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

        // Lean target for the player's bike (FUN_14019d360, the branch taken when bike + 0x268 is
        // set): a piecewise stick curve, 24 deg at 0.4, 45 deg at 0.7, 80 deg at full stick.
        let a = i.stick.abs().min(1.0);
        let curve = if a < 0.4 {
            a / 0.4 * 24.0
        } else if a < 0.7 {
            (a - 0.4) / 0.3 * 21.0 + 24.0
        } else {
            (a - 0.7) / 0.3 * 35.0 + 45.0
        };
        let mut target = curve * sign(i.stick);
        if i.lean_help & 2 == 0 {
            if i.speed < 6.0 {
                target *= i.speed / 6.0;
            }
        } else {
            let mu = if i.loose_surface { i.grip * 1.4 } else { i.grip };
            if i.stick >= 0.0 {
                target = target.min(Self::max_lean(i, mu, true));
            } else {
                target = target.max(-Self::max_lean(i, mu, false));
            }
        }
        if i.lean_help & 1 == 0 {
            self.target_lean = target;
        } else {
            // Blend towards the current lean as the front unloads, then rate limit: leaning in at
            // up to 180 deg/s (scaled by the stick), returning at leanhelp_scale * 180 deg/s.
            let half_weight = i.weight * 0.5;
            let f = if i.lean_help & 2 == 0 {
                (i.front_load.max(0.0) / half_weight).sqrt() * 0.8 + 0.2
            } else if i.front_load >= half_weight {
                1.0
            } else {
                let side = if i.stick < 0.0 { -Self::max_lean(i, i.grip, false) } else { Self::max_lean(i, i.grip, true) };
                let r = (self.target_lean / side).clamp(-1.0, 1.0);
                let f0 = (1.0 - r.abs().sqrt()) * 0.2;
                if i.front_load > 0.0 { (i.front_load / half_weight).sqrt() * (1.0 - f0) + f0 } else { f0 }
            };
            let desired = (1.0 - f) * i.lean + f * target;
            let speed_factor = if i.speed >= 20.0 {
                1.0 - ((i.speed - 20.0) * 0.015).atan() / std::f32::consts::FRAC_PI_2
            } else {
                1.0
            };
            let base = i.lean_help_scale * speed_factor * 180.0;
            let prev = self.target_lean;
            let (out, mult) = if desired >= 0.0 {
                if desired > prev { (true, i.stick) } else { (false, 0.0) }
            } else if prev <= desired {
                (true, 0.0)
            } else {
                (false, i.stick.abs())
            };
            let step = dt * ((speed_factor * 180.0 - base) * mult + base);
            self.target_lean = if out { (prev + step).min(desired) } else { (prev - step).max(desired) };
        }

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

        // Hand over to the centring PD as the front wheel unloads (wheelies, jumps).
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
        self.blend = blend;
        p + d + ig + yaw
    }
}
