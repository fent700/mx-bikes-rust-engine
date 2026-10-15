//! Engine, clutch and sequential gearbox driven by the stock `.cfg` / `.engn` data.
//!
//! Crank -> primary reduction -> clutch basket | clutch | gearbox input -> gear -> sprockets -> wheel.
//! The clutch is a torque-limited coupling between the basket (crank / primary) and the gearbox
//! input (wheel * final * gear); it slips whenever the transmitted torque exceeds its capacity.

use mxbikes_formats::bike_def::{BikeDefinition, DrivelineDefinition, EngineDefinition, GearboxDefinition};

const RPM_TO_RAD: f32 = std::f32::consts::TAU / 60.0;

#[derive(Debug, Clone)]
pub struct Powertrain {
    pub engine: EngineDefinition,
    pub gearbox: GearboxDefinition,
    pub driveline: DrivelineDefinition,
    pub autoclutch_rpm: (f32, f32),

    /// Crank speed (rad/s).
    pub crank_omega: f32,
    /// 0 = neutral, 1..N.
    pub gear: usize,
    pub throttle: f32,
    /// Rider lever: 0 released (engaged) .. 1 pulled.
    pub clutch_lever: f32,
    pub auto_clutch: bool,
    /// Automatic gearbox assist driven by the bike's `autoshift` rpm table.
    pub auto_shift: bool,
    pub stalled: bool,
    pub starter: bool,
    pub limiter_cut: bool,
    shift_timer: f32,
    pending_gear: Option<usize>,
    /// Torque delivered to the rear wheel on the last step (Nm).
    pub wheel_torque: f32,
    /// Bike road speed (m/s) and rear tyre radius (m), for the autoshift speed checks.
    pub road_speed: f32,
    pub wheel_radius: f32,
    /// The rear wheel is off the ground (autoshift upshift speed allowance x0.7).
    pub rear_airborne: bool,
    pub clutch_slip: f32,
}

impl Powertrain {
    pub fn new(def: &BikeDefinition) -> Self {
        Self {
            engine: def.engine.clone(),
            gearbox: def.gearbox.clone(),
            driveline: def.driveline.clone(),
            autoclutch_rpm: def.autoclutch_rpm,
            crank_omega: def.engine.idle_rpm * RPM_TO_RAD,
            gear: 0,
            throttle: 0.0,
            clutch_lever: 0.0,
            auto_clutch: true,
            auto_shift: false,
            stalled: false,
            starter: false,
            limiter_cut: false,
            shift_timer: 0.0,
            pending_gear: None,
            wheel_torque: 0.0,
            clutch_slip: 0.0,
            road_speed: 0.0,
            wheel_radius: 0.33,
            rear_airborne: false,
        }
    }

    /// Road speed (m/s) at `rpm` in gear `gear` with the clutch locked.
    fn speed_at(&self, rpm: f32, gear: usize) -> f32 {
        let ratio = self.gearbox.ratios.get(gear.wrapping_sub(1)).copied().unwrap_or(0.0);
        let total = self.engine.primary_ratio.max(1e-3) * self.driveline.final_ratio() * ratio;
        if total <= 0.0 {
            return 0.0;
        }
        rpm * RPM_TO_RAD / total * self.wheel_radius
    }

    /// The game's autoshift (`FUN_1401ad410`): out of neutral into first once the engine passes
    /// the neutral `up` speed; up a gear past `up` when the road speed confirms it (so wheel spin
    /// does not shift; x0.7 with the rear wheel airborne); down below `down` from second gear up
    /// when the road speed is below that gear's `down` speed. First never shifts back to neutral.
    /// INFERRED: the effective tyre radius is taken upright (the game scales it with lean).
    fn autoshift(&mut self) {
        let rpm = self.rpm();
        let Some(&(down, up)) = self.gearbox.autoshift.get(self.gear) else { return };
        let g = self.gear;
        if down.is_none_or(|d| d <= rpm) {
            if let Some(u) = up.filter(|&u| u < rpm) {
                if g < self.gear_count() {
                    if g == 0 {
                        self.shift_up();
                        return;
                    }
                    let mut v = self.speed_at(u, g);
                    if self.rear_airborne {
                        v *= 0.7;
                    }
                    if v < self.road_speed {
                        self.shift_up();
                    }
                }
            }
        } else if g > 1 {
            let v = self.speed_at(down.unwrap_or(0.0), g);
            if self.road_speed < v {
                self.shift_down();
            }
        }
    }

    pub fn reset(&mut self) {
        self.crank_omega = self.engine.idle_rpm * RPM_TO_RAD;
        self.gear = 0;
        self.pending_gear = None;
        self.shift_timer = 0.0;
        self.throttle = 0.0;
        self.clutch_lever = 0.0;
        self.stalled = false;
        self.starter = false;
        self.limiter_cut = false;
        self.wheel_torque = 0.0;
        self.clutch_slip = 0.0;
    }

    /// The starter is turning the engine (button held, below the starter's top speed).
    pub fn cranking(&self) -> bool {
        self.starter && self.rpm() < self.engine.starter_max_rpm
    }

    pub fn rpm(&self) -> f32 {
        self.crank_omega / RPM_TO_RAD
    }

    pub fn gear_count(&self) -> usize {
        self.gearbox.ratios.len()
    }

    /// Gearbox output (rear sprocket) to clutch basket ratio for the engaged gear; 0 in neutral.
    fn gear_ratio(&self) -> f32 {
        if self.pending_gear.is_some() || self.gear == 0 {
            return 0.0;
        }
        self.gearbox.ratios.get(self.gear - 1).copied().unwrap_or(0.0)
    }

    pub fn shift_in_progress(&self) -> bool {
        self.pending_gear.is_some()
    }

    /// Sequential box: requests are ignored while a shift is still in progress.
    pub fn shift_up(&mut self) {
        if self.pending_gear.is_some() {
            return;
        }
        let target = (self.gear + 1).min(self.gear_count());
        self.request(target, self.gearbox.upshift_time);
    }

    pub fn shift_down(&mut self) {
        if self.pending_gear.is_some() {
            return;
        }
        let target = self.gear.saturating_sub(1);
        self.request(target, self.gearbox.downshift_time);
    }

    fn request(&mut self, target: usize, time: f32) {
        if target != self.gear {
            self.pending_gear = Some(target);
            self.shift_timer = time.max(0.0);
        }
    }

    fn curve(points: &[(f32, f32)], rpm: f32) -> f32 {
        match points {
            [] => 0.0,
            [only] => only.1,
            _ => {
                if rpm <= points[0].0 {
                    return points[0].1;
                }
                for w in points.windows(2) {
                    if rpm <= w[1].0 {
                        let t = (rpm - w[0].0) / (w[1].0 - w[0].0).max(1e-3);
                        return w[0].1 + (w[1].1 - w[0].1) * t;
                    }
                }
                points[points.len() - 1].1
            }
        }
    }

    /// Net crank torque from combustion (scaled by effective throttle) and friction/pumping.
    pub fn crank_torque(&self, throttle: f32) -> f32 {
        if self.stalled {
            return 0.0;
        }
        let rpm = self.rpm();
        let full = Self::curve(&self.engine.torque_curve, rpm);
        let drag = Self::curve(&self.engine.braking_curve, rpm);
        let t = if self.limiter_cut { 0.0 } else { throttle };
        full * t + drag * (1.0 - t)
    }

    /// Clutch engagement 0..1 from lever, dead zone and the auto clutch rpm window.
    fn engagement(&self) -> f32 {
        let dz = self.engine.clutch.dead_zone.clamp(0.0, 0.95);
        let lever = ((self.clutch_lever - dz) / (1.0 - dz)).clamp(0.0, 1.0);
        let mut e = 1.0 - lever;
        if self.auto_clutch {
            let (lo, hi) = self.autoclutch_rpm;
            let a = ((self.rpm() - lo) / (hi - lo).max(1.0)).clamp(0.0, 1.0);
            e = e.min(a);
        }
        e
    }

    /// Advances the drivetrain. `wheel_omega` is the rear wheel spin (rad/s, positive forward).
    /// Returns the torque applied to the rear wheel (Nm).
    pub fn step(&mut self, dt: f32, wheel_omega: f32) -> f32 {
        if let Some(target) = self.pending_gear {
            self.shift_timer -= dt;
            if self.shift_timer <= 0.0 {
                self.gear = target;
                self.pending_gear = None;
            }
        }

        if self.auto_shift && self.pending_gear.is_none() && !self.stalled {
            self.autoshift();
        }
        let rpm = self.rpm();
        if rpm >= self.engine.limiter_rpm {
            self.limiter_cut = true;
        } else if rpm < self.engine.limiter_rpm - 150.0 {
            self.limiter_cut = false;
        }

        if self.stalled {
            if self.starter {
                self.stalled = false;
                self.crank_omega = self.crank_omega.max(self.engine.starter_max_rpm * 0.5 * RPM_TO_RAD);
            } else {
                self.crank_omega = (self.crank_omega - 400.0 * dt).max(0.0);
                self.wheel_torque = 0.0;
                return 0.0;
            }
        }

        // The throttle never closes below the idle opening; idle speed is where that opening's
        // combustion torque balances the friction curve.
        let eff_throttle = self.throttle.clamp(0.0, 1.0).max(self.engine.idle_throttle);
        let mut crank = self.crank_torque(eff_throttle);
        if self.starter && rpm < self.engine.starter_max_rpm {
            crank += self.engine.starter_torque;
        }

        let primary = self.engine.primary_ratio.max(1e-3);
        let final_ratio = self.driveline.final_ratio();
        let gear = self.gear_ratio();
        let basket_omega = self.crank_omega / primary;
        let input_omega = wheel_omega * final_ratio * gear;

        let engagement = if gear > 0.0 { self.engagement() } else { 0.0 };
        let capacity = self.engine.clutch.max_torque * engagement;
        let crank_inertia = self.engine.inertia + self.driveline.first_axis_inertia;
        let basket_inertia = crank_inertia * primary * primary;

        // Torque needed at the basket to lock both sides within this step, limited by capacity.
        let slip = basket_omega - input_omega;
        let lock_torque = slip * basket_inertia / dt.max(1e-5);
        let clutch_torque = lock_torque.clamp(-capacity, capacity);
        self.clutch_slip = slip;

        let crank_net = crank - clutch_torque / primary;
        self.crank_omega += crank_net / crank_inertia * dt;
        self.crank_omega = self.crank_omega.max(0.0);

        let stall_omega = self.engine.stall_rpm * RPM_TO_RAD;
        if self.crank_omega < stall_omega && engagement > 0.5 && !self.starter {
            self.stalled = true;
            self.crank_omega = 0.0;
        }

        let breaking = self.engine.breaking_rpm * RPM_TO_RAD;
        if self.crank_omega > breaking {
            self.crank_omega = breaking;
        }

        self.wheel_torque = clutch_torque * gear * final_ratio * self.driveline.efficiency;
        self.wheel_torque
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxbikes_formats::ConfigNode;

    fn bike() -> BikeDefinition {
        let cfg = ConfigNode::parse(
            "engine\n{\nInertia = 0.006\nIdleRPM = 2300\nIdleThrottle = 0.079\nLimiter0 = 14300\nRatio0 = 24, 73\nstall\n{\nRPM = 1600\n}\nClutch\n{\nMaxTorque = 105\nDeadZone = 0.25\n}\n}\ngearbox\n{\ngearing\n{\nratio0 = 13, 32\nratio1 = 16, 32\n}\nNumGears = 2\nGear0 = 0\nGear1 = 1\n}\n",
        )
        .unwrap();
        // Stock 250xf.engn curves (x in rev/s).
        let engn = ConfigNode::parse(concat!(
            "torque\n{\nnumpoints = 11\npoint0 = 0.625, 0.230062\npoint1 = 33.75, 14.378834\n",
            "point2 = 66.875, 15.912578\npoint3 = 99.375, 18.788345\npoint4 = 141.25, 26.380367\n",
            "point5 = 166.875015, 25.11503\npoint6 = 199.999985, 23.38957\npoint7 = 233.125015, 19.93865\n",
            "point8 = 251.25, 5.559817\npoint9 = 266.875, 2.108894\npoint10 = 298.125, 0\n}\n",
            "brake\n{\nnumpoints = 8\npoint0 = 0, 0\npoint1 = 33.75, -1.15031\npoint2 = 66.25, -2.300613\n",
            "point3 = 99.375, -4.409507\npoint4 = 133.125, -6.901838\npoint5 = 178.749985, -11.119634\n",
            "point6 = 223.125, -16.296015\npoint7 = 300, -27.077257\n}\n"
        ))
        .unwrap();
        BikeDefinition::from_nodes(&cfg, None, Some(&engn), None)
    }

    #[test]
    fn idles_in_neutral() {
        let mut p = Powertrain::new(&bike());
        for _ in 0..1000 {
            p.step(0.002, 0.0);
        }
        assert!(!p.stalled);
        assert!((p.rpm() - 2300.0).abs() < 300.0, "idle rpm {}", p.rpm());
    }

    #[test]
    fn free_revs_to_limiter() {
        let mut p = Powertrain::new(&bike());
        p.throttle = 1.0;
        for _ in 0..1000 {
            p.step(0.002, 0.0);
        }
        assert!(p.rpm() > 13500.0 && p.rpm() < 14600.0, "rpm {}", p.rpm());
    }

    #[test]
    fn stalls_against_locked_wheel_without_autoclutch() {
        let mut p = Powertrain::new(&bike());
        p.auto_clutch = false;
        p.gear = 1;
        for _ in 0..500 {
            p.step(0.002, 0.0);
        }
        assert!(p.stalled);
    }
}
