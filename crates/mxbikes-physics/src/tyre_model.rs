//! Motorcycle Magic Formula tyre (MF-MC, Pacejka "Tyre and Vehicle Dynamics" ch. 11) driven by
//! the coefficient names stored in stock `.tyre` files (`p_Cx1`, `p_Ky6`, `r_Bx1`, `q_Dz1`, ...).
//!
//! Lateral force carries camber thrust as a separate arctan term:
//!   Fy0 = Dy sin(Cy atan(By a - Ey(By a - atan(By a))) + Cg atan(Bg g - Eg(Bg g - atan(Bg g))))
//! Combined slip uses the cosine weighting functions Gxa / Gyk.

use mxbikes_formats::tyre_def::{SurfaceGrip, TyreDefinition};

#[derive(Debug, Clone)]
pub struct TyreModel {
    pub def: TyreDefinition,
    c: Coefs,
}

#[derive(Debug, Clone, Copy, Default)]
struct Coefs {
    fz0: f32,
    r0: f32,
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
    r_cx1: f32,
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
    r_by1: f32,
    r_by2: f32,
    r_by3: f32,
    r_cy1: f32,
    r_hy1: f32,
    p_hy1: f32,
    q_sy1: f32,
    q_bz1: f32,
    q_cz1: f32,
    q_dz1: f32,
    q_dz8: f32,
    q_ez1: f32,
}

/// Forces in the contact frame: `fx` along the rolling direction, `fy` lateral (towards the
/// wheel's left when positive, i.e. along `contact_lateral`), `mz` aligning moment about the normal.
#[derive(Debug, Clone, Copy, Default)]
pub struct TyreForces {
    pub fx: f32,
    pub fy: f32,
    pub mz: f32,
}

#[inline]
fn mf(b: f32, c: f32, d: f32, e: f32, x: f32) -> f32 {
    let bx = b * x;
    d * (c * (bx - e * (bx - bx.atan())).atan()).sin()
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
            r_cx1: k("r_Cx1"),
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
            r_by1: k("r_By1"),
            r_by2: k("r_By2"),
            r_by3: k("r_By3"),
            r_cy1: k("r_Cy1"),
            r_hy1: k("r_Hy1"),
            p_hy1: k("p_Hy1"),
            q_sy1: k("q_sy1"),
            q_bz1: k("q_Bz1"),
            q_cz1: k("q_Cz1"),
            q_dz1: k("q_Dz1"),
            q_dz8: k("q_Dz8"),
            q_ez1: k("q_Ez1"),
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

    /// `kappa`: longitudinal slip ratio, `alpha`: slip angle (rad), `gamma`: camber (rad),
    /// `grip`: surface friction scaling.
    pub fn forces(&self, fz: f32, kappa: f32, alpha: f32, gamma: f32, grip: f32) -> TyreForces {
        if fz <= 0.0 {
            return TyreForces::default();
        }
        let c = &self.c;
        let dfz = (fz - c.fz0) / c.fz0;

        // Pure longitudinal.
        let cx = c.p_cx1.max(0.1);
        let dx = (c.p_dx1 + c.p_dx2 * dfz) * grip * fz;
        let kx = fz * (c.p_kx1 + c.p_kx2 * dfz) * (c.p_kx3 * dfz).exp();
        let bx = kx / (cx * dx).max(1e-3);
        let ex = ((c.p_ex1 + c.p_ex2 * dfz + c.p_ex3 * dfz * dfz) * (1.0 - c.p_ex4 * kappa.signum())).min(1.0);
        // Rolling resistance enters as a longitudinal slip shift (q_sy1), as in the game.
        let kappa_x = kappa - c.q_sy1 * fz / kx.max(1e-3);
        let fx0 = mf(bx, cx, dx, ex, kappa_x);

        // Pure lateral with camber thrust.
        let cy = c.p_cy1.max(0.1);
        // MF-MC (Pacejka 6.1 motorcycle) lateral terms, matching the game's bike tyre evaluator.
        let g2 = gamma * gamma;
        let mu_y = c.p_dy1 * (c.p_dy2 * dfz).exp() / (1.0 + c.p_dy3 * g2) * grip;
        let dy = mu_y * fz;
        let ky_a = c.p_ky1 * c.fz0
            * (c.p_ky2 * (fz / ((c.p_ky3 + c.p_ky4 * g2).max(1e-4) * c.fz0)).atan()).sin()
            / (1.0 + c.p_ky5 * g2);
        let ky_g = (c.p_ky6 + c.p_ky7 * dfz) * fz;
        let by = ky_a / (cy * dy + 0.1);
        let alpha_y = alpha + c.p_hy1;
        let ey = (c.p_ey1 + c.p_ey2 * g2 + alpha_y.signum() * (c.p_ey3 + c.p_ey4 * gamma)).min(1.0);
        let cg = c.p_cy2;
        let bg = if cg.abs() > 1e-4 { ky_g / (cg * dy + 0.1) } else { 0.0 };
        let eg = c.p_ey5.min(1.0);
        let ba = by * alpha_y;
        let bgg = bg * gamma;
        let fy0 = dy
            * (cy * (ba - ey * (ba - ba.atan())).atan() + cg * (bgg - eg * (bgg - bgg.atan())).atan()).sin();

        // Combined slip weighting.
        let bxa = c.r_bx1 * (c.r_bx2 * kappa).atan().cos();
        let gxa = if c.r_bx1 > 0.0 {
            (c.r_cx1 * (bxa * alpha).atan()).cos()
        } else {
            1.0
        };
        let byk = c.r_by1 * (c.r_by2 * (alpha - c.r_by3)).atan().cos();
        let gyk = if c.r_by1 > 0.0 {
            let num = (c.r_cy1 * (byk * (kappa + c.r_hy1)).atan()).cos();
            let den = (c.r_cy1 * (byk * c.r_hy1).atan()).cos();
            num / den.max(1e-3)
        } else {
            1.0
        };

        let fx = fx0 * gxa.max(0.0);
        let fy = fy0 * gyk.max(0.0);

        // Aligning moment: pneumatic trail on the slip-angle force plus camber twisting.
        let dt = fz * (c.r0 / c.fz0) * c.q_dz1;
        let t = dt * (c.q_cz1 * (c.q_bz1 * alpha - c.q_ez1 * (c.q_bz1 * alpha - (c.q_bz1 * alpha).atan())).atan()).cos();
        let fy_alpha = dy * (cy * (ba - ey * (ba - ba.atan())).atan()).sin() * gyk.max(0.0);
        let mz = -t * fy_alpha + fz * c.r0 * c.q_dz8 * gamma * gxa.max(0.0) * 0.1;

        TyreForces { fx, fy, mz }
    }

    /// Rolling resistance force magnitude (N) on a surface.
    pub fn rolling_resistance(&self, fz: f32, surface: &SurfaceGrip, wet: f32) -> f32 {
        let r = surface.resistance_dry + (surface.resistance_wet - surface.resistance_dry) * wet;
        fz.max(0.0) * r * 0.01
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxbikes_formats::ConfigNode;

    const REAR: &str = "R0 = 0.34\nFz0 = 1000\np_Cx1 = 1.0\np_Dx1 = 0.78\np_Dx2 = -0.06\np_Kx1 = 30\nr_Bx1 = 13\nr_Bx2 = -10\nr_Cx1 = 1.05\np_Cy1 = 1.2\np_Cy2 = 0.6\np_Dy1 = 0.75\np_Dy2 = -0.2\np_Ky1 = 20\np_Ky2 = 1\np_Ky3 = 0.01\np_Ky6 = 3\nr_By1 = 7\nr_By2 = 8\nr_Cy1 = 1.05\nr_Hy1 = 0.04\nq_Bz1 = 9\nq_Cz1 = 1.1\nq_Dz1 = 0.1\nq_Ez1 = -10\nRadius = 0.325\nTorusRadius = 0.075\nspringBase = 60000\nspringFactor = 500\n";

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
}
