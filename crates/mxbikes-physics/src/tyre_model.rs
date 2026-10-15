//! Motorcycle Magic Formula tyre, ported from the game's bike tyre evaluator
//! (FUN_1401e2a90, with pure lateral FUN_1401e2500, combined lateral FUN_1401e2890 and the
//! slip-stiffness helper FUN_1401e21d0).
//!
//! The game copies the `.tyre` coefficients into a packed float array and evaluates the
//! MF-MC equations on it. Every term below is one the evaluator computes, in the same form,
//! including its clamps (0.01 floors, E <= 1) and its `+ 0.1` in the B denominators.
//!
//! Four slots of the array are written per wheel per step from the surface table
//! (FUN_1401a2fb0 -> FUN_1401b92d0): the longitudinal and lateral friction scales (surface grip),
//! the lateral shape scale (`lat_stiffness`, 1 when absent) and the rolling resistance factor
//! (`resistance_dry` / `resistance_wet`).
//!
//! Coefficient names for slots that are zero in every stock `.tyre` file (the p_Hx, p_Vx, r_Vy,
//! q_Hz, q_Ez2-5, q_Dz2-4, q_Dz6-11 except q_Dz8, s_Sz2-4, q_sx groups) are INFERRED from the
//! standard MF ordering of the array; they do not affect stock content.

use mxbikes_formats::tyre_def::{SurfaceGrip, TyreDefinition};
use std::f32::consts::FRAC_2_PI;

#[derive(Debug, Clone)]
pub struct TyreModel {
    pub def: TyreDefinition,
    c: Coefs,
}

#[derive(Debug, Clone, Copy, Default)]
struct Coefs {
    fz0: f32,
    r0: f32,
    // Longitudinal.
    p_cx1: f32,
    p_dx1: f32,
    p_dx2: f32,
    p_ex1: f32,
    p_ex2: f32,
    p_ex3: f32,
    p_ex4: f32,
    p_kx1: f32,
    p_kx2: f32,
    p_kx3: f32,
    r_bx1: f32,
    r_bx2: f32,
    r_bx3: f32,
    r_cx1: f32,
    r_hx1: f32,
    // Lateral.
    p_cy1: f32,
    p_cy2: f32,
    p_dy1: f32,
    p_dy2: f32,
    p_dy3: f32,
    p_ey1: f32,
    p_ey2: f32,
    p_ey3: f32,
    p_ey4: f32,
    p_ey5: f32,
    p_ky1: f32,
    p_ky2: f32,
    p_ky3: f32,
    p_ky4: f32,
    p_ky5: f32,
    p_ky6: f32,
    p_ky7: f32,
    p_hy1: f32,
    r_by1: f32,
    r_by2: f32,
    r_by3: f32,
    r_by4: f32,
    r_cy1: f32,
    r_hy1: f32,
    r_vy1: f32,
    r_vy2: f32,
    r_vy3: f32,
    r_vy4: f32,
    r_vy5: f32,
    r_vy6: f32,
    // Aligning moment.
    q_bz1: f32,
    q_bz2: f32,
    q_bz3: f32,
    q_bz5: f32,
    q_bz6: f32,
    q_bz9: f32,
    q_bz10: f32,
    q_cz1: f32,
    q_dz1: f32,
    q_dz2: f32,
    q_dz3: f32,
    q_dz4: f32,
    q_dz6: f32,
    q_dz7: f32,
    q_dz8: f32,
    q_dz9: f32,
    q_dz10: f32,
    q_dz11: f32,
    q_ez1: f32,
    q_ez2: f32,
    q_ez3: f32,
    q_ez4: f32,
    q_ez5: f32,
    q_hz1: f32,
    q_hz2: f32,
    q_hz3: f32,
    q_hz4: f32,
    s_sz1: f32,
    s_sz2: f32,
    s_sz3: f32,
    s_sz4: f32,
    // Overturning and rolling resistance moments.
    q_sx1: f32,
    q_sx2: f32,
    q_sx3: f32,
    q_sy1: f32,
    q_sy2: f32,
}

/// Inputs of one evaluation, as the game packs them for FUN_1401e2a90.
#[derive(Debug, Clone, Copy, Default)]
pub struct TyreInput {
    pub fz: f32,
    /// Longitudinal slip ratio.
    pub kappa: f32,
    /// Slip angle (rad).
    pub alpha: f32,
    /// Camber (rad).
    pub gamma: f32,
    /// Surface friction scale (both directions).
    pub grip: f32,
    /// Lateral shape scale (`lat_stiffness`, 1 for stock tyres).
    pub lat_shape: f32,
    /// Surface rolling resistance factor (`resistance_dry` / `resistance_wet`).
    pub resistance: f32,
    /// Rolling speed of the tread (m/s): effective radius times spin.
    pub roll_speed: f32,
}

/// Forces in the contact frame: `fx` along the rolling direction, `fy` lateral (towards the
/// wheel's left when positive, i.e. along `contact_lateral`), `mz` aligning moment about the
/// normal, `mx` overturning moment, `my` rolling resistance moment about the spin axis (opposes
/// a positive roll speed). `mu_x` / `mu_y` are the friction coefficients used.
#[derive(Debug, Clone, Copy, Default)]
pub struct TyreForces {
    pub fx: f32,
    pub fy: f32,
    pub mz: f32,
    pub mx: f32,
    pub my: f32,
    pub mu_x: f32,
    pub mu_y: f32,
}

/// Effective slip stiffnesses B*C*D (FUN_1401e21d0), used by the low-speed slip terms.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlipStiffness {
    pub long: f32,
    pub lat: f32,
    pub camber: f32,
}

/// Result of the pure + combined lateral evaluation (FUN_1401e2500 + FUN_1401e2890).
#[derive(Debug, Clone, Copy, Default)]
struct Lateral {
    cy: f32,
    ky_a: f32,
    by: f32,
    sv_yk: f32,
    fy: f32,
}

#[inline]
fn sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// `atan(x - (x - atan x) * e)`: the curvature-shaped arctangent of the Magic Formula.
#[inline]
fn shaped(x: f32, e: f32) -> f32 {
    (x - (x - x.atan()) * e).atan()
}

impl TyreModel {
    pub fn new(def: TyreDefinition) -> Self {
        let k = |n: &str| def.coef(n);
        let c = Coefs {
            fz0: def.nominal_load.max(1.0),
            r0: def.unloaded_radius,
            p_cx1: k("p_Cx1"),
            p_dx1: k("p_Dx1"),
            p_dx2: k("p_Dx2"),
            p_ex1: k("p_Ex1"),
            p_ex2: k("p_Ex2"),
            p_ex3: k("p_Ex3"),
            p_ex4: k("p_Ex4"),
            p_kx1: k("p_Kx1"),
            p_kx2: k("p_Kx2"),
            p_kx3: k("p_Kx3"),
            r_bx1: k("r_Bx1"),
            r_bx2: k("r_Bx2"),
            r_bx3: k("r_Bx3"),
            r_cx1: k("r_Cx1"),
            r_hx1: k("r_Hx1"),
            p_cy1: k("p_Cy1"),
            p_cy2: k("p_Cy2"),
            p_dy1: k("p_Dy1"),
            p_dy2: k("p_Dy2"),
            p_dy3: k("p_Dy3"),
            p_ey1: k("p_Ey1"),
            p_ey2: k("p_Ey2"),
            p_ey3: k("p_Ey3"),
            p_ey4: k("p_Ey4"),
            p_ey5: k("p_Ey5"),
            p_ky1: k("p_Ky1"),
            p_ky2: k("p_Ky2"),
            p_ky3: k("p_Ky3"),
            p_ky4: k("p_Ky4"),
            p_ky5: k("p_Ky5"),
            p_ky6: k("p_Ky6"),
            p_ky7: k("p_Ky7"),
            p_hy1: k("p_Hy1"),
            r_by1: k("r_By1"),
            r_by2: k("r_By2"),
            r_by3: k("r_By3"),
            r_by4: k("r_By4"),
            r_cy1: k("r_Cy1"),
            r_hy1: k("r_Hy1"),
            r_vy1: k("r_Vy1"),
            r_vy2: k("r_Vy2"),
            r_vy3: k("r_Vy3"),
            r_vy4: k("r_Vy4"),
            r_vy5: k("r_Vy5"),
            r_vy6: k("r_Vy6"),
            q_bz1: k("q_Bz1"),
            q_bz2: k("q_Bz2"),
            q_bz3: k("q_Bz3"),
            q_bz5: k("q_Bz5"),
            q_bz6: k("q_Bz6"),
            q_bz9: k("q_Bz9"),
            q_bz10: k("q_Bz10"),
            q_cz1: k("q_Cz1"),
            q_dz1: k("q_Dz1"),
            q_dz2: k("q_Dz2"),
            q_dz3: k("q_Dz3"),
            q_dz4: k("q_Dz4"),
            q_dz6: k("q_Dz6"),
            q_dz7: k("q_Dz7"),
            q_dz8: k("q_Dz8"),
            q_dz9: k("q_Dz9"),
            q_dz10: k("q_Dz10"),
            q_dz11: k("q_Dz11"),
            q_ez1: k("q_Ez1"),
            q_ez2: k("q_Ez2"),
            q_ez3: k("q_Ez3"),
            q_ez4: k("q_Ez4"),
            q_ez5: k("q_Ez5"),
            q_hz1: k("q_Hz1"),
            q_hz2: k("q_Hz2"),
            q_hz3: k("q_Hz3"),
            q_hz4: k("q_Hz4"),
            s_sz1: k("s_Sz1"),
            s_sz2: k("s_Sz2"),
            s_sz3: k("s_Sz3"),
            s_sz4: k("s_Sz4"),
            q_sx1: k("q_sx1"),
            q_sx2: k("q_sx2"),
            q_sx3: k("q_sx3"),
            q_sy1: k("q_sy1"),
            q_sy2: k("q_sy2"),
        };
        Self { def, c }
    }

    pub fn radius(&self) -> f32 {
        self.def.radius
    }

    pub fn torus_radius(&self) -> f32 {
        self.def.torus_radius
    }

    /// Vertical carcass stiffness (N/m) at the configured pressure.
    pub fn vertical_stiffness(&self) -> f32 {
        self.def.spring_base + self.def.spring_factor * self.def.pressure_kpa
    }

    /// Clamped longitudinal slip stiffness Kx and friction coefficient mu_x.
    fn long_terms(&self, fz: f32, dfz: f32, grip: f32) -> (f32, f32) {
        let c = &self.c;
        let kx = ((dfz * c.p_kx3).exp() * ((dfz * c.p_kx2 + c.p_kx1) * fz)).max(0.01);
        let mu_x = ((dfz * c.p_dx2 + c.p_dx1) * grip).max(0.01);
        (kx, mu_x)
    }

    /// Pure lateral (FUN_1401e2500) followed by the combined-slip weighting (FUN_1401e2890).
    fn lateral(&self, fz: f32, dfz: f32, kappa: f32, alpha: f32, gamma: f32, grip: f32, lat_shape: f32) -> (Lateral, f32) {
        let c = &self.c;
        let g2 = gamma * gamma;
        let mu_y = ((dfz * c.p_dy2).exp() * c.p_dy1 * grip / (g2 * c.p_dy3 + 1.0)).max(0.01);
        let cy = (lat_shape * c.p_cy1).max(0.01);
        let cg = c.p_cy2.max(0.01);
        let dy = mu_y * fz;
        let ky_a = (c.p_ky2 * (fz / ((g2 * c.p_ky4 + c.p_ky3) * c.fz0)).atan()).sin() * (c.fz0 * c.p_ky1)
            / (g2 * c.p_ky5 + 1.0);
        let by = ky_a / (dy * cy + 0.1);
        let bg = ((dfz * c.p_ky7 + c.p_ky6) * fz) / (dy * cg + 0.1);
        let alpha_y = c.p_hy1 + alpha;
        let ey = (sign(alpha_y) * (gamma * c.p_ey4 + c.p_ey3) + g2 * c.p_ey2 + c.p_ey1).min(1.0);
        let eg = c.p_ey5.min(1.0);
        let fy0 = (shaped(bg * gamma, eg) * cg + shaped(by * alpha_y, ey) * cy).sin() * dy;

        let sv_yk = (alpha * c.r_vy4).atan().cos()
            * ((dfz * c.r_vy2 + c.r_vy1 + gamma * c.r_vy3) * mu_y * fz)
            * (c.r_vy5 * (kappa * c.r_vy6).atan()).sin();
        let byk = ((alpha - c.r_by3) * c.r_by2).atan().cos() * (g2 * c.r_by4 + c.r_by1);
        let byk = byk.max(0.01);
        let gyk = (c.r_cy1 * (byk * (c.r_hy1 + kappa)).atan()).cos() / (c.r_cy1 * (byk * c.r_hy1).atan()).cos();
        let fy = gyk * fy0 + sv_yk;
        (Lateral { cy, ky_a, by, sv_yk, fy }, mu_y)
    }

    /// The game's bike tyre evaluation (FUN_1401e2a90).
    pub fn evaluate(&self, i: TyreInput) -> TyreForces {
        let c = &self.c;
        let fz = i.fz.max(0.0);
        if fz <= 0.0 {
            return TyreForces::default();
        }
        let dfz = (fz - c.fz0) / c.fz0;
        let (kappa, alpha, gamma) = (i.kappa, i.alpha, i.gamma);
        let grip = i.grip;

        // Longitudinal. The vertical shift S_Vx is multiplied by zero in the game; the slip
        // shift is the rolling resistance coefficient q_sy1 (times Fz) over Kx.
        let (kx, mu_x) = self.long_terms(fz, dfz, grip);
        let dx = mu_x * fz;
        let kappa_x = kappa - (c.q_sy1 * fz) / kx;
        let ex = ((1.0 - sign(kappa_x) * c.p_ex4) * (dfz * c.p_ex2 + c.p_ex1 + dfz * c.p_ex3 * dfz)).min(1.0);
        let bx = kx / (dx * c.p_cx1 + 0.1);
        let fx0 = (shaped(bx * kappa_x, ex) * c.p_cx1).sin() * dx;
        let bxa = ((kappa * c.r_bx2).atan().cos() * (gamma * c.r_bx3 * gamma + c.r_bx1)).max(0.01);
        let gxa = (c.r_cx1 * ((c.r_hx1 + alpha) * bxa).atan()).cos() / (c.r_cx1 * (c.r_hx1 * bxa).atan()).cos();
        let fx = gxa * fx0;

        // Lateral at the actual camber, and again at zero camber for the trail term.
        let (lat, mu_y) = self.lateral(fz, dfz, kappa, alpha, gamma, grip, i.lat_shape);
        let (lat0, _) = self.lateral(fz, dfz, kappa, alpha, 0.0, grip, i.lat_shape);
        let fy = lat.fy;

        // Aligning moment.
        let lambda = grip;
        let abs_g = gamma.abs();
        let bt = (((dfz * c.q_bz2 + c.q_bz1 + dfz * c.q_bz3 * dfz) * (gamma * c.q_bz6 * gamma + c.q_bz5 * abs_g + 1.0))
            / lambda)
            .max(0.01);
        let ct = c.q_cz1.max(0.01);
        let et = (((gamma * c.q_ez5 + c.q_ez4) * (bt * ct * alpha).atan() * FRAC_2_PI + 1.0)
            * (dfz * c.q_ez2 + c.q_ez1 + dfz * c.q_ez3 * dfz))
            .min(1.0);
        let alpha_t = (dfz * c.q_hz4 + c.q_hz3) * gamma + dfz * c.q_hz2 + c.q_hz1 + alpha;
        let kx_ky = kx / (lat.ky_a + 0.1);
        let k2 = kx_ky * kx_ky * kappa * kappa;
        let cos_a = alpha.cos();
        let r0_fz = c.r0 * fz;

        let t_arg = sign(alpha) * (alpha * alpha + k2).sqrt() * bt;
        let dt = (dfz * c.q_dz2 + c.q_dz1) * (c.r0 / c.fz0) * fz * (gamma * c.q_dz4 * gamma + c.q_dz3 * abs_g + 1.0);
        let trail = (shaped(t_arg, et) * ct).cos() * dt * cos_a;

        let br = lat.by * c.q_bz10 * lat.cy + c.q_bz9 / lambda;
        let r_arg = sign(alpha_t) * (alpha_t * alpha_t + k2).sqrt() * br;
        let dr = ((dfz * c.q_dz9 + c.q_dz8) * gamma + dfz * c.q_dz7 + c.q_dz6)
            + (dfz * c.q_dz11 + c.q_dz10) * gamma * abs_g;
        let mzr = r_arg.atan().cos() * (dr * r0_fz * cos_a * lambda);

        let s = ((fy / c.fz0) * c.s_sz2 + c.s_sz1 + (dfz * c.s_sz4 + c.s_sz3) * gamma) * c.r0;
        let mz = mzr - trail * (lat0.fy - lat.sv_yk) + s * fx;

        let mx = ((c.q_sx1 - gamma * c.q_sx2) + (fy * c.q_sx3) / c.fz0) * r0_fz;
        let my = ((i.roll_speed / 16.667).atan() * c.q_sy1 + (fx * c.q_sy2) / c.fz0) * -r0_fz * i.resistance;

        TyreForces { fx, fy, mz, mx, my, mu_x, mu_y }
    }

    /// Pure-slip evaluation with stock shape and no rolling resistance.
    pub fn forces(&self, fz: f32, kappa: f32, alpha: f32, gamma: f32, grip: f32) -> TyreForces {
        self.evaluate(TyreInput { fz, kappa, alpha, gamma, grip, lat_shape: 1.0, resistance: 0.0, roll_speed: 0.0 })
    }

    /// Effective slip stiffnesses B*C*D at zero camber (FUN_1401e21d0).
    pub fn slip_stiffness(&self, fz: f32, grip: f32, lat_shape: f32) -> SlipStiffness {
        let c = &self.c;
        let fz = fz.max(0.0);
        let dfz = (fz - c.fz0) / c.fz0;
        let dx = ((dfz * c.p_dx2 + c.p_dx1) * grip).max(0.01) * fz;
        let kx = (dfz * c.p_kx3).exp() * ((dfz * c.p_kx2 + c.p_kx1) * fz);
        let long = (kx / (dx * c.p_cx1 + 0.1) * c.p_cx1 * dx).max(0.01);
        let dy = ((dfz * c.p_dy2 + c.p_dy1) * grip).max(0.01) * fz;
        let cy = (lat_shape * c.p_cy1).max(0.01);
        let ky_a = (c.p_ky2 * (fz / (c.p_ky3 * c.fz0)).atan()).sin() * (c.fz0 * c.p_ky1);
        let lat = (ky_a / (dy * cy + 0.1) * cy * dy).max(0.01);
        let cg = c.p_cy2.max(0.01);
        let camber = ((dfz * c.p_ky7 + c.p_ky6) * fz) / (cg * dy + 0.1) * cg * dy;
        SlipStiffness { long, lat, camber }
    }

    /// Peak longitudinal slip 3 Dx / (B C D) at the nominal load and unit grip, as the game
    /// stores it when the bike is set up (FUN_140196500 via FUN_1401e21d0).
    pub fn kappa_peak(&self) -> f32 {
        let fz = self.c.fz0;
        let dx = (self.c.p_dx1.max(0.01)) * fz;
        3.0 * dx / self.slip_stiffness(fz, 1.0, 1.0).long
    }

    /// Surface grip and rolling resistance for a wetness in 0..1 on a non-deformable surface
    /// (FUN_1401b92d0, second branch).
    pub fn surface_terms(surface: &SurfaceGrip, wet: f32) -> (f32, f32) {
        let grip = (surface.wet_grip - surface.dry_grip) * wet + surface.dry_grip;
        let resistance = (surface.resistance_wet - surface.resistance_dry) * wet + surface.resistance_dry;
        (grip, resistance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxbikes_formats::ConfigNode;

    const REAR: &str = "R0 = 0.34\nFz0 = 1000\np_Cx1 = 1.0\np_Dx1 = 0.78\np_Dx2 = -0.06\np_Kx1 = 30\nr_Bx1 = 13\nr_Bx2 = -10\nr_Cx1 = 1.05\np_Cy1 = 1.2\np_Cy2 = 0.6\np_Dy1 = 0.75\np_Dy2 = -0.2\np_Ky1 = 20\np_Ky2 = 1\np_Ky3 = 0.01\np_Ky6 = 3\nr_By1 = 7\nr_By2 = 8\nr_Cy1 = 1.05\nr_Hy1 = 0.04\nq_Bz1 = 9\nq_Cz1 = 1.1\nq_Dz1 = 0.1\nq_Dz8 = 0.1\nq_Ez1 = -10\nq_sy1 = 0.03\nRadius = 0.325\nTorusRadius = 0.075\nspringBase = 60000\nspringFactor = 500\n";

    fn tyre() -> TyreModel {
        TyreModel::new(TyreDefinition::from_node(&ConfigNode::parse(REAR).unwrap()))
    }

    #[test]
    fn peak_longitudinal_matches_dx() {
        let t = tyre();
        let peak = (1..200)
            .map(|i| t.forces(1000.0, i as f32 * 0.005, 0.0, 0.0, 1.0).fx)
            .fold(0.0f32, f32::max);
        assert!((peak - 780.0).abs() < 20.0, "peak {peak}");
    }

    #[test]
    fn rolling_resistance_shift_and_moment() {
        let t = tyre();
        // q_sy1 shifts the slip curve: zero slip gives a braking force of about q_sy1 * Fz.
        let f = t.forces(1000.0, 0.0, 0.0, 0.0, 1.0);
        assert!(f.fx < -20.0 && f.fx > -40.0, "fx at zero slip {}", f.fx);
        // My = -R0 Fz res q_sy1 atan(v / 16.667).
        let m = t.evaluate(TyreInput { fz: 1000.0, grip: 1.0, lat_shape: 1.0, resistance: 3.0, roll_speed: 16.667, ..Default::default() });
        let expect = -0.34 * 1000.0 * 3.0 * 0.03 * 1.0f32.atan();
        assert!((m.my - expect).abs() < 0.1, "my {} vs {expect}", m.my);
    }

    #[test]
    fn camber_thrust_and_slip_have_consistent_sign() {
        let t = tyre();
        let from_camber = t.forces(1000.0, 0.0, 0.0, 0.3, 1.0).fy;
        let from_slip = t.forces(1000.0, 0.0, 0.05, 0.0, 1.0).fy;
        assert!(from_camber > 0.0 && from_slip > 0.0);
        // Camber stiffness ~ p_Ky6 * Fz per radian at small angles.
        let small = t.forces(1000.0, 0.0, 0.0, 0.01, 1.0).fy / 0.01;
        assert!((small - 3000.0).abs() < 300.0, "camber stiffness {small}");
    }

    #[test]
    fn combined_slip_reduces_lateral() {
        let t = tyre();
        let pure = t.forces(1000.0, 0.0, 0.1, 0.0, 1.0).fy;
        let comb = t.forces(1000.0, 0.2, 0.1, 0.0, 1.0).fy;
        assert!(comb < pure * 0.8);
    }

    #[test]
    fn aligning_torque_opposes_slip_angle() {
        let t = tyre();
        let f = t.forces(1000.0, 0.0, 0.05, 0.0, 1.0);
        assert!(f.fy > 0.0);
        assert!(f.mz < 0.0, "aligning torque must oppose slip angle: mz={}", f.mz);
    }

    #[test]
    fn camber_moment_is_full_q_dz8_term() {
        let t = tyre();
        // At zero slip the trail term vanishes; Mz is the residual Fz R0 q_Dz8 gamma.
        let f = t.forces(1000.0, 0.0, 0.0, 0.5, 1.0);
        let expect = 1000.0 * 0.34 * 0.1 * 0.5;
        assert!((f.mz - expect).abs() < 1.0, "mz {} vs {expect}", f.mz);
    }
}
