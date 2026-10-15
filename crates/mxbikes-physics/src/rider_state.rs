//! Rider body states the game keeps next to the bike (rider update `FUN_1401a8480`), for a rider
//! on the bike in the normal riding mode. They drive the rider animation weights, the rider's
//! physical mass displacement, leg spring dynamics, and crash detachment.
//!
//! Ported from the original engine (`FUN_1401a8480` and `rider.prm` via `FUN_140192ec0`):
//! - Commanded stand amount (+2/s up, -4/s down, -2/s airborne toward speed-ramped button).
//! - Physical sprung leg extension (0..0.18 m, `legs/SeatSpring`, `legs/StandSpring`, `legs/CompDamping`,
//!   `legs/ExtDamping`), driving the actual physical body `stand` (0..1 = `leg_pos / 0.18 m`).
//! - Dynamic torso pitch target (`0x22e30`): ground pitch relative to terrain, wheelie / stoppie
//!   limits, jump pitch limits, slewed by `prm.pitch_speed` (5.0/s) into `fore_aft` (`0x22e38`).
//! - Dynamic torso roll lean: auto L/R lean with high-speed attenuation, slewed by `prm.lean_speed` (2.0/s).
//! - Rider pose angles (`pitch_deg`, `roll_deg`, `steer_rot_deg`) matching lines 1165-1209.
//! - Dynamic rider mass displacement in chassis frame.
//! - 25-frame rolling reaction force buffer and crash detachment (lines 1489-1499).
//! - Stability help restoring torques (lines 2095-2275).

use crate::motorcycle::RiderInput;
use glam::{Vec2, Vec3};
use mxbikes_formats::RiderPrm;

/// Default slew rate for torso lean from `rider.prm` (`LeanSpeed = 2.0`, read at +0x23780 by `FUN_1401a8480` line 325).
pub const LEAN_SPEED_PRM: f32 = 2.0;
/// Default slew rate for fore/aft pitch from `rider.prm` (`PitchSpeed = 5.0`, read at +0x23784 by `FUN_1401a8480` line 413).
pub const PITCH_SPEED_PRM: f32 = 5.0;

#[derive(Debug, Clone)]
pub struct RiderState {
    /// Rider physics and spring parameters loaded from `rider.prm` (`FUN_140192ec0`).
    pub params: RiderPrm,
    /// Commanded stand amount 0..1 (+0x22e10).
    pub stand_cmd: f32,
    /// Standing amount of the rider body 0..1 (+0x22e18 = `leg_pos / 0.18`).
    pub stand: f32,
    /// Physical vertical leg extension in metres (0.0 = seated, 0.18 = full stand).
    pub leg_pos: f32,
    /// Vertical leg extension velocity (m/s).
    pub leg_vel: f32,
    /// Airborne blend 0..1 (+0x233f4).
    pub airborne: f32,
    /// Torso lean target -1..1 (+0x22e48).
    pub torso_target: f32,
    /// Torso lean -1 left .. 1 right (+0x22e50).
    pub torso: f32,
    /// Target pitch -1 back .. 1 forward (+0x22e30).
    pub fore_aft_target: f32,
    /// Fore/aft pitch -1 back .. 1 forward (+0x22e38).
    pub fore_aft: f32,
    /// Instantaneous torso pitch angle in degrees (+0x22e64).
    pub pitch_deg: f32,
    /// Instantaneous torso roll angle in degrees (+0x22e6c).
    pub roll_deg: f32,
    /// Instantaneous steer rotation angle in degrees (+0x22e68).
    pub steer_rot_deg: f32,
    /// Profile aid `autoriderlrlean` (inferred mapping of flag +0x23994 bit 0): the steer input is
    /// added to the lateral lean input.
    pub auto_lr_lean: bool,
    /// Torso lean slew rate (+0x23780 from `rider.prm`).
    pub lean_speed: f32,
    /// Fore/aft pitch slew rate (+0x23784 from `rider.prm`).
    pub pitch_speed: f32,
    /// 25-sample rolling reaction force history for crash detachment (`FUN_1401a8480` lines 1435-1499).
    force_history: [Vec3; 25],
    force_hist_idx: usize,
    force_hist_count: usize,
}

impl Default for RiderState {
    fn default() -> Self {
        let params = RiderPrm::default();
        let lean_speed = params.lean_speed;
        let pitch_speed = params.pitch_speed;
        Self {
            params,
            stand_cmd: 0.0,
            stand: 0.0,
            leg_pos: 0.0,
            leg_vel: 0.0,
            airborne: 0.0,
            torso_target: 0.0,
            torso: 0.0,
            fore_aft_target: 0.0,
            fore_aft: 0.0,
            pitch_deg: 0.0,
            roll_deg: 0.0,
            steer_rot_deg: 0.0,
            auto_lr_lean: true,
            lean_speed,
            pitch_speed,
            force_history: [Vec3::ZERO; 25],
            force_hist_idx: 0,
            force_hist_count: 0,
        }
    }
}

fn slew(value: &mut f32, target: f32, rate: f32, dt: f32) {
    let step = rate * dt;
    *value += (target - *value).clamp(-step, step);
}

impl RiderState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_params(params: RiderPrm) -> Self {
        let mut s = Self::default();
        s.set_params(params);
        s
    }

    pub fn set_params(&mut self, params: RiderPrm) {
        self.lean_speed = params.lean_speed;
        self.pitch_speed = params.pitch_speed;
        self.params = params;
    }

    pub fn reset(&mut self) {
        let params = self.params.clone();
        let auto_lr = self.auto_lr_lean;
        *self = Self::with_params(params);
        self.auto_lr_lean = auto_lr;
    }

    /// Advances rider states by `dt`. Returns the rider mass offset `(lateral, longitudinal)` in metres.
    pub fn step(
        &mut self,
        dt: f32,
        input: &RiderInput,
        speed: f32,
        lean_deg: f32,
        pitch_deg: f32,
        fwd_accel: f32,
        airborne_now: bool,
    ) -> Vec2 {
        let air_flag = if airborne_now { 1.0 } else { 0.0 };
        slew(&mut self.airborne, air_flag, 4.0, dt);

        // 1. Stand target = button * speed ramp * lean factor (FUN_1401a8480).
        let ramp = if airborne_now {
            1.0
        } else if speed < 1.5 {
            0.0
        } else {
            ((speed - 1.5) / 1.5).min(1.0)
        };
        let lean_factor = if !airborne_now && lean_deg.abs() >= 30.0 {
            (1.0 - (lean_deg.abs() - 27.0) / 18.0).max(0.0)
        } else {
            1.0
        };
        let stand_target = if input.stand { ramp * lean_factor } else { 0.0 };
        let down_rate = if airborne_now { 2.0 } else { 4.0 };
        let rate = if stand_target > self.stand_cmd { 2.0 } else { down_rate };
        slew(&mut self.stand_cmd, stand_target, rate, dt);

        // 2. Physical sprung legs (FUN_1401a8480 lines 1350-1400)
        // Full standing travel is 0.18 m; body stand amount 0..1 = leg_pos / 0.18.
        let target_leg_pos = self.stand_cmd * 0.18;
        let k_legs = (self.params.legs_stand_spring - self.params.legs_seat_spring) * self.stand + self.params.legs_seat_spring;
        let c_legs = 2.0 * (k_legs * 80.0).sqrt() * if self.leg_vel >= 0.0 { self.params.legs_ext_damping } else { self.params.legs_comp_damping };
        let f_spring = (k_legs * (target_leg_pos - self.leg_pos) - c_legs * self.leg_vel).clamp(0.0, 2943.0);
        self.leg_vel += (f_spring / 80.0) * dt;
        self.leg_pos = (self.leg_pos + self.leg_vel * dt).clamp(0.0, 0.18);
        self.stand = (self.leg_pos / 0.18).clamp(0.0, 1.0);

        // 3. Dynamic pitch target calculation (FUN_1401a8480 lines 970-1105)
        let prm = &self.params;
        let mut target_pitch = if !airborne_now {
            let max_angle = if pitch_deg >= 0.0 {
                prm.standing_pitch_max_angle_forward * self.stand + prm.sitting_pitch_max_angle_forward * (1.0 - self.stand)
            } else {
                prm.standing_pitch_max_angle_backward * self.stand + prm.sitting_pitch_max_angle_backward * (1.0 - self.stand)
            };
            let pitch_rate = prm.standing_pitch_rate * self.stand + prm.sitting_pitch_rate * (1.0 - self.stand);
            let mut t = (fwd_accel / pitch_rate.max(1.0)) + (pitch_deg / max_angle.max(1.0));
            // Wheelie / stoppie compensation (lines 1010-1044)
            if pitch_deg <= 0.0 {
                t += fwd_accel / prm.stoppie_pitch_rate.max(1.0) + pitch_deg / prm.stoppie_pitch_max_angle.abs().max(1.0);
            } else {
                t += fwd_accel / prm.wheelie_pitch_rate.max(1.0) + pitch_deg / prm.wheelie_pitch_max_angle.abs().max(1.0);
            }
            t
        } else {
            // Airborne jump pitch (lines 1068-1073)
            -pitch_deg / prm.jump_pitch_max_angle.max(1.0) + fwd_accel / prm.jump_pitch_rate.max(1.0)
        };
        target_pitch = target_pitch.clamp(-1.0, 1.0);
        // Modulated by rider manual lean forward/backward input (lines 1090-1105)
        let cmd_pitch = input.lean_forward.clamp(-1.0, 1.0);
        if cmd_pitch <= 0.0 {
            target_pitch -= (-1.0 - target_pitch) * cmd_pitch;
        } else {
            target_pitch = (1.0 - target_pitch) * cmd_pitch + target_pitch;
        }
        self.fore_aft_target = target_pitch.clamp(-1.0, 1.0);
        slew(&mut self.fore_aft, self.fore_aft_target, self.pitch_speed, dt);

        // 4. Torso lateral lean (FUN_1401a8480 lines 112-124 / 651-665)
        if self.auto_lr_lean {
            let target = (input.lean_lateral + input.steer).clamp(-1.0, 1.0);
            let mut rate = 2.0;
            if speed >= 20.0 {
                rate = (1.0 - ((speed - 20.0) * 0.015).atan() / std::f32::consts::FRAC_PI_2) * 2.0;
            }
            slew(&mut self.torso_target, target, rate, dt);
        } else {
            self.torso_target = input.lean_lateral.clamp(-1.0, 1.0);
        }
        slew(&mut self.torso, self.torso_target, self.lean_speed, dt);

        // 5. Rider pose angles in degrees (FUN_1401a8480 lines 1165-1209)
        if self.fore_aft <= 0.0 {
            self.pitch_deg = prm.sitting_pitch_backward * self.fore_aft * (1.0 - self.stand)
                + prm.standing_pitch_backward * self.fore_aft * self.stand;
        } else {
            self.pitch_deg = prm.sitting_pitch_forward * self.fore_aft * (1.0 - self.stand)
                + prm.standing_pitch_forward * self.fore_aft * self.stand;
        }
        self.roll_deg = (lean_deg / 90.0) * prm.sitting_lean * (1.0 - self.stand) - prm.lean * self.torso;
        self.steer_rot_deg = if prm.steer_rot != 0.0 { input.steer * -0.5 } else { 0.0 };

        // 6. Dynamic rider CG offset in chassis frame (metres: X right, longitudinal)
        let lat_offset = self.torso * 0.12;
        let long_offset = (self.pitch_deg / 45.0) * 0.15 + prm.standing_pos * self.stand;
        Vec2::new(lat_offset, long_offset)
    }

    /// 25-frame rolling reaction force buffer check (FUN_1401a8480 lines 1489-1499).
    /// Returns true if rider detaches (crash).
    pub fn update_reaction_forces(&mut self, reaction_force: Vec3) -> bool {
        self.force_history[self.force_hist_idx] = reaction_force;
        self.force_hist_idx = (self.force_hist_idx + 1) % 25;
        if self.force_hist_count < 25 {
            self.force_hist_count += 1;
        }

        // Lateral force limit: 4000 N
        if reaction_force.x.abs() > 4000.0 {
            return true;
        }
        // Longitudinal force limit: 10000 N
        if reaction_force.z.abs() > 10000.0 {
            return true;
        }
        // Vertical compression limit: 15000 N
        if reaction_force.y > 15000.0 {
            return true;
        }
        // 25-sample moving average vertical extension limit: -2000 N
        if self.force_hist_count >= 25 {
            let sum_y: f32 = self.force_history.iter().map(|f| f.y).sum();
            let avg_y = sum_y / 25.0;
            if avg_y < -2000.0 {
                return true;
            }
        }
        false
    }

    /// Restoring torques from `stabilityhelp` aid (FUN_1401a8480 lines 2095-2275).
    /// Scale is profile `stabilityhelp_scale`. Returns restoring torque in chassis frame.
    pub fn stability_torque(
        &self,
        roll_deg: f32,
        roll_rate_deg: f32,
        pitch_deg: f32,
        pitch_rate_deg: f32,
        yaw_deg: f32,
        yaw_rate_deg: f32,
        scale: f32,
    ) -> Vec3 {
        if scale <= 0.0 {
            return Vec3::ZERO;
        }
        let prm = &self.params;
        let t_roll = -(roll_rate_deg * prm.stability_roll_rate + roll_deg * prm.stability_roll_factor) * scale;
        let t_pitch = -(pitch_rate_deg * prm.stability_pitch_rate + pitch_deg * prm.stability_pitch_factor) * scale;
        let yaw_mult = if (yaw_deg < 0.0 && yaw_rate_deg > 0.0) || (yaw_deg > 0.0 && yaw_rate_deg < 0.0) {
            2.0
        } else {
            1.0
        };
        let t_yaw = -(yaw_rate_deg * prm.stability_yaw_rate * yaw_mult + yaw_deg * prm.stability_yaw_factor) * scale;
        Vec3::new(t_pitch, t_yaw, -t_roll)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stand_rises_at_two_per_second_and_drops_at_four() {
        let mut s = RiderState::new();
        let mut input = RiderInput { stand: true, ..Default::default() };
        for _ in 0..25 {
            s.step(0.01, &input, 10.0, 0.0, 0.0, 0.0, false);
        }
        assert!((s.stand_cmd - 0.5).abs() < 1e-4);
        input.stand = false;
        for _ in 0..5 {
            s.step(0.01, &input, 10.0, 0.0, 0.0, 0.0, false);
        }
        assert!((s.stand_cmd - 0.3).abs() < 1e-4);
    }

    #[test]
    fn torso_target_follows_lean_plus_steer_with_aid() {
        let mut s = RiderState::new();
        let input = RiderInput { steer: 0.5, lean_lateral: 0.3, ..Default::default() };
        for _ in 0..100 {
            s.step(0.01, &input, 10.0, 0.0, 0.0, 0.0, false);
        }
        assert!((s.torso_target - 0.8).abs() < 1e-4);
    }

    #[test]
    fn sprung_legs_lag_behind_stand_cmd() {
        let mut s = RiderState::new();
        let input = RiderInput { stand: true, ..Default::default() };
        s.step(0.1, &input, 10.0, 0.0, 0.0, 0.0, false);
        // Commanded stand has moved up to 0.2
        assert!(s.stand_cmd > 0.15);
        // Physical body stand lags behind due to leg spring/damper
        assert!(s.stand < s.stand_cmd);
    }

    #[test]
    fn crash_detachment_triggers_on_excessive_lateral_force() {
        let mut s = RiderState::new();
        assert!(!s.update_reaction_forces(Vec3::new(1000.0, 1000.0, 1000.0)));
        assert!(s.update_reaction_forces(Vec3::new(4500.0, 1000.0, 1000.0)));
    }
}

