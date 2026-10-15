//! Multibody motocross bike.
//!
//! Bodies: chassis + rider (6 DoF rigid body, composite inertia of every part at the reference
//! pose), steering head rotation, fork slider travel, swingarm rotation, front and rear wheel spin
//! and the drivetrain. Internal DoFs are integrated with their own inertia; their accelerations are
//! fed back to the chassis as reaction forces/torques so momentum is exchanged consistently.
//!
//! Frame: right-handed engine frame, +X right, +Y up, -Z forward. Hard points come from `.geom`
//! (asset frame, converted by mirroring Z). The chassis local origin is the asset origin, i.e. the
//! ground point under the bike in the authored pose.

use crate::powertrain::Powertrain;
use crate::rider_state::RiderState;
use crate::steer_control::{SteerController, SteerInputs};
use crate::suspension::{Fork, RearLinkage, Shock};
use crate::tyre_model::TyreModel;
use glam::{Mat3, Quat, Vec2, Vec3};
use mxbikes_formats::bike_def::BikeDefinition;
use mxbikes_formats::reader::lh_to_rh;
use mxbikes_formats::tyre_def::TyreDefinition;
use mxbikes_formats::Surface;

pub const GRAVITY: f32 = 9.81;
const AIR_DENSITY: f32 = 1.2;
const SUBSTEP: f32 = 1.0 / 500.0;
const RIDER_MASS: f32 = 80.0;
const RELAX_LONG: f32 = 0.12;
const RELAX_LAT: f32 = 0.05;
/// Maps the original controller's output onto this engine's steer angle convention.
const STEER_OUTPUT_SIGN: f32 = -1.0;

/// Terrain queries used by the simulation (engine frame).
pub trait Ground {
    fn height(&self, x: f32, z: f32) -> f32;
    fn normal(&self, x: f32, z: f32) -> Vec3;
    fn surface(&self, _x: f32, _z: f32) -> Surface {
        Surface::Soil
    }
    /// Tyre deformation input at (x, z): `amount` in metres before the surface factor.
    fn deform(&self, _x: f32, _z: f32, _amount: f32) {}
}

/// Flat ground at y = 0, used by tests and the empty scene.
pub struct FlatGround;

impl Ground for FlatGround {
    fn height(&self, _x: f32, _z: f32) -> f32 {
        0.0
    }
    fn normal(&self, _x: f32, _z: f32) -> Vec3 {
        Vec3::Y
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RiderInput {
    /// -1 left .. 1 right.
    pub steer: f32,
    pub throttle: f32,
    pub front_brake: f32,
    pub rear_brake: f32,
    pub clutch: f32,
    /// -1 back .. 1 forward (rider weight shift).
    pub lean_forward: f32,
    /// -1 left .. 1 right (rider lateral torso lean).
    pub lean_lateral: f32,
    /// True when the rider stands on the footpegs.
    pub stand: bool,
    pub shift_up: bool,
    pub shift_down: bool,
    pub starter: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WheelTelemetry {
    pub load: f32,
    pub slip_ratio: f32,
    pub slip_angle: f32,
    pub camber: f32,
    pub fx: f32,
    pub fy: f32,
    pub contact: Option<Vec3>,
    pub surface: Option<Surface>,
    /// Tyre surface speed at the rim (spin rate * radius, m/s).
    pub rim_speed: f32,
    /// Wheel centre velocity (m/s).
    pub velocity: Vec3,
    /// Ground normal and rolling direction at the contact.
    pub normal: Vec3,
    pub rolling: Vec3,
}

#[derive(Debug, Clone)]
struct Wheel {
    tyre: TyreModel,
    mass: f32,
    spin_inertia: f32,
    /// Spin rate (rad/s), positive when rolling forward.
    omega: f32,
    /// Accumulated spin angle for rendering.
    angle: f32,
    slip_long: f32,
    slip_lat: f32,
    last_penetration: f32,
    brake_torque_max: f32,
    grip_scale: f32,
    telemetry: WheelTelemetry,
    /// Previous step contact point for difference-based rut speed (FUN_1401a2fb0 line 700).
    last_contact: Option<Vec3>,
}

pub fn dirt_level(amount: f32) -> u32 {
    if amount <= 0.1666 {
        0
    } else if amount <= 0.5 {
        1
    } else if amount <= 0.8333 {
        2
    } else {
        3
    }
}

impl Wheel {
    fn new(def: TyreDefinition, brake_torque_max: f32, grip_scale: f32) -> Self {
        let spin_inertia = def.inertia.x.max(0.1);
        let mass = def.mass;
        Self {
            tyre: TyreModel::new(def),
            mass,
            spin_inertia,
            omega: 0.0,
            angle: 0.0,
            slip_long: 0.0,
            slip_lat: 0.0,
            last_penetration: 0.0,
            brake_torque_max,
            grip_scale,
            telemetry: WheelTelemetry::default(),
            last_contact: None,
        }
    }
}

/// Result of a tyre evaluation: world force at the contact and the spin torque about the axle.
struct ContactResult {
    force: Vec3,
    point: Vec3,
    aligning: Vec3,
    road_torque: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BodyContact {
    pub point: Vec3,
    pub normal: Vec3,
    pub normal_force: f32,
    pub scrape_speed: f32,
    pub impact_impulse: f32,
}

#[derive(Debug, Clone)]
pub struct Motorcycle {
    pub def: BikeDefinition,

    // Chassis rigid body (centre of mass state).
    pub com: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
    com_local: Vec3,
    mass: f32,
    inertia_local: Mat3,
    inertia_local_inv: Mat3,

    // Hard points (chassis local, engine frame).
    head: Vec3,
    rake: f32,
    front_upper: Vec3,
    fwheel: Vec3,
    pivot: Vec3,
    front_sprocket: Vec3,
    rider_point: Vec3,
    aero_point: Vec3,

    // Internal DoFs.
    pub steer_angle: f32,
    pub steer_rate: f32,
    steer_inertia: f32,
    pub fork_compression: f32,
    pub fork_speed: f32,
    fork_mass: f32,
    pub swingarm_angle: f32,
    pub swingarm_rate: f32,
    swingarm_inertia: f32,
    swingarm_mass: f32,

    fork: Fork,
    linkage: RearLinkage,
    shock: Shock,
    front: Wheel,
    rear: Wheel,
    pub powertrain: Powertrain,

    pub input: RiderInput,
    /// When false the bars are left free (riderless stability tests).
    pub rider_steering: bool,
    /// Scale applied to the cfg steering controller gains.
    pub steer_gain_scale: f32,
    steer_ctrl: SteerController,
    /// Smoothed rider body offset (lateral, longitudinal), metres in chassis frame.
    pub rider_offset: Vec2,
    /// Rider body states (stand, airborne, torso lean, fore/aft) as the game tracks them.
    pub rider: RiderState,
    pub crashed: bool,
    /// Profile aid `stabilityhelp_scale` (restoring torques, `FUN_1401a8480` lines 2095-2275).
    pub stability_help_scale: f32,
    /// Profile aid `brakehelp` (anti-loop & anti-lock).
    pub brake_help: bool,
    /// Profile aid `tractionhelp` (slip-ratio throttle attenuation).
    pub traction_help: bool,
    last_accel: Vec3,
    accumulator: f32,
    rut_accum: f32,

    pub speed_kmh: f32,
    pub lean_angle_deg: f32,
    pub dirt_factor: f32,
    pub rider_dirt_factor: f32,
    pub last_body_contact: Option<BodyContact>,
}

fn box_inertia(m: f32, size: Vec3) -> Vec3 {
    Vec3::new(
        m / 12.0 * (size.y * size.y + size.z * size.z),
        m / 12.0 * (size.x * size.x + size.z * size.z),
        m / 12.0 * (size.x * size.x + size.y * size.y),
    )
}

fn parallel_axis(m: f32, r: Vec3) -> Mat3 {
    let d = r.length_squared();
    Mat3::from_diagonal(Vec3::splat(d * m)) - m * Mat3::from_cols(r * r.x, r * r.y, r * r.z)
}

impl Motorcycle {
    pub fn new(def: BikeDefinition, tyres: (TyreDefinition, TyreDefinition), spawn: Vec3, heading: f32) -> Self {
        let g = &def.geometry;
        let head = lh_to_rh(g.chassis_steer);
        let pivot = lh_to_rh(g.swingarm_pivot);
        let fork = Fork::new(def.front_suspension.clone(), g.front_length);
        let linkage = RearLinkage::new(g, def.rear_suspension.rod_length_adjust);
        let shock = Shock {
            def: def.rear_suspension.clone(),
        };

        // `wheelN { Friction }` scales the tyre's surface grip.
        let front = Wheel::new(tyres.0, def.brakes[0].max_torque(), def.brakes[0].wheel_friction);
        let rear = Wheel::new(tyres.1, def.brakes[1].max_torque(), def.brakes[1].wheel_friction);

        // Composite mass properties at the authored pose.
        let rake = g.rake_deg.to_radians();
        let steer_rot = Quat::from_rotation_x(rake);
        let fwheel_ref = head + steer_rot * (lh_to_rh(g.front_upper) + Vec3::Y * (-g.front_length) + lh_to_rh(g.fwheel));
        let rwheel_ref = pivot + Vec3::new(0.0, g.rwheel.y, -g.rwheel.z);
        let parts: [(f32, Vec3, Vec3); 7] = [
            (def.chassis.mass, lh_to_rh(g.chassis_mass), def.chassis.inertia),
            (RIDER_MASS, lh_to_rh(g.rider_mass), Vec3::new(7.7, 2.7, 8.3)),
            (def.steer.mass, head + steer_rot * lh_to_rh(g.steer_mass), box_inertia(def.steer.mass, def.steer.inertia_box)),
            (def.front_suspension.mass, fwheel_ref, Vec3::splat(0.05)),
            (front.mass, fwheel_ref, Vec3::new(front.spin_inertia, front.spin_inertia * 0.5, front.spin_inertia * 0.5)),
            (def.rear_suspension.mass, pivot + Vec3::new(0.0, g.rsusp_mass.y, -g.rsusp_mass.z), Vec3::splat(0.3)),
            (rear.mass, rwheel_ref, Vec3::new(rear.spin_inertia, rear.spin_inertia * 0.5, rear.spin_inertia * 0.5)),
        ];
        let mass: f32 = parts.iter().map(|p| p.0).sum();
        let com_local = parts.iter().map(|p| p.0 * p.1).sum::<Vec3>() / mass;
        let mut inertia = Mat3::ZERO;
        for (m, p, i) in parts {
            inertia += Mat3::from_diagonal(i) + parallel_axis(m, p - com_local);
        }

        let steer_inertia = box_inertia(def.steer.mass, def.steer.inertia_box).y
            + front.spin_inertia * 0.5
            + (front.mass + def.front_suspension.mass) * 0.03 * 0.03;
        let arm = linkage.axle_arm.length();
        let swingarm_inertia = def.rear_suspension.mass * (arm * 0.5).powi(2) + rear.mass * arm * arm + 0.2;

        let rotation = Quat::from_rotation_y(heading);
        let mut bike = Self {
            com: spawn + rotation * com_local,
            rotation,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            com_local,
            mass,
            inertia_local: inertia,
            inertia_local_inv: inertia.inverse(),
            head,
            rake,
            front_upper: lh_to_rh(g.front_upper),
            fwheel: lh_to_rh(g.fwheel),
            pivot,
            front_sprocket: lh_to_rh(g.front_sprocket),
            rider_point: lh_to_rh(g.rider_mass),
            aero_point: lh_to_rh(g.pressure_center),
            steer_angle: 0.0,
            steer_rate: 0.0,
            steer_inertia,
            fork_compression: g.front_length * 0.3,
            fork_speed: 0.0,
            fork_mass: def.front_suspension.mass + front.mass,
            swingarm_angle: linkage.angle_range.0 + (linkage.angle_range.1 - linkage.angle_range.0) * 0.3,
            swingarm_rate: 0.0,
            swingarm_inertia,
            swingarm_mass: def.rear_suspension.mass + rear.mass,
            powertrain: Powertrain::new(&def),
            fork,
            linkage,
            shock,
            front,
            rear,
            input: RiderInput::default(),
            rider_steering: true,
            steer_gain_scale: 1.0,
            steer_ctrl: SteerController::default(),
            rider_offset: Vec2::ZERO,
            rider: RiderState::new(),
            crashed: false,
            stability_help_scale: 0.0,
            brake_help: false,
            traction_help: false,
            last_accel: Vec3::ZERO,
            accumulator: 0.0,
            rut_accum: 0.0,
            speed_kmh: 0.0,
            lean_angle_deg: 0.0,
            dirt_factor: 0.0,
            rider_dirt_factor: 0.0,
            last_body_contact: None,
            def,
        };
        bike.place_on(spawn, heading);
        bike
    }

    /// Positions the bike upright with both tyres just touching flat ground at `ground` height.
    pub fn place_on(&mut self, ground: Vec3, heading: f32) {
        self.rotation = Quat::from_rotation_y(heading);
        self.velocity = Vec3::ZERO;
        self.angular_velocity = Vec3::ZERO;
        self.steer_angle = 0.0;
        self.steer_rate = 0.0;
        self.fork_compression = self.def.geometry.front_length * 0.3;
        self.swingarm_angle = self.linkage.angle_range.0 + (self.linkage.angle_range.1 - self.linkage.angle_range.0) * 0.3;
        self.fork_speed = 0.0;
        self.swingarm_rate = 0.0;
        self.front.omega = 0.0;
        self.rear.omega = 0.0;
        self.front.slip_long = 0.0;
        self.front.slip_lat = 0.0;
        self.rear.slip_long = 0.0;
        self.rear.slip_lat = 0.0;
        self.front.last_penetration = 0.0;
        self.rear.last_penetration = 0.0;
        self.front.last_contact = None;
        self.rear.last_contact = None;
        self.rut_accum = 0.0;
        self.crashed = false;
        self.steer_ctrl.reset();
        self.rider_offset = Vec2::ZERO;
        self.rider.reset();
        self.input = RiderInput::default();
        self.powertrain.reset();
        // Dirt stays on a reset; the garage's Clean button removes it.
        let lowest = (self.front_axle_local().y - self.front.tyre.radius())
            .min(self.rear_axle_local().y - self.rear.tyre.radius());
        let origin = ground - Vec3::Y * lowest;
        self.com = origin + self.rotation * self.com_local;
    }

    /// Sets a straight-line forward speed with matching wheel spin.
    pub fn set_speed(&mut self, v: f32) {
        let forward = self.rotation * Vec3::NEG_Z;
        self.velocity = forward * v;
        self.front.omega = v / self.front.tyre.radius();
        self.rear.omega = v / self.rear.tyre.radius();
        self.front.slip_long = 0.0;
        self.rear.slip_long = 0.0;
    }

    /// Returns current linear speed in m/s.
    pub fn speed(&self) -> f32 {
        self.velocity.length()
    }

    /// Returns current speed in km/h.
    pub fn speed_kmh(&self) -> f32 {
        self.speed_kmh
    }

    // ----- Kinematics (chassis local) -----

    fn steer_frame(&self) -> Quat {
        let axis = Quat::from_rotation_x(self.rake) * Vec3::Y;
        Quat::from_axis_angle(axis, -self.steer_angle) * Quat::from_rotation_x(self.rake)
    }

    pub fn steer_axis_local(&self) -> Vec3 {
        Quat::from_rotation_x(self.rake) * Vec3::Y
    }

    /// Fork slider (lower leg) origin in the steering frame.
    pub fn fork_slider_offset(&self) -> Vec3 {
        self.front_upper + Vec3::Y * (self.fork_compression - self.fork.travel)
    }

    pub fn front_axle_local(&self) -> Vec3 {
        self.head + self.steer_frame() * (self.fork_slider_offset() + self.fwheel)
    }

    fn rear_axle_side(&self) -> Vec2 {
        self.linkage.axle(self.swingarm_angle)
    }

    pub fn rear_axle_local(&self) -> Vec3 {
        let a = self.rear_axle_side();
        self.pivot + Vec3::new(0.0, a.x, -a.y)
    }

    fn to_world(&self, local: Vec3) -> Vec3 {
        self.com + self.rotation * (local - self.com_local)
    }

    fn point_velocity(&self, world: Vec3) -> Vec3 {
        self.velocity + self.angular_velocity.cross(world - self.com)
    }

    // ----- Public transforms for rendering -----

    /// World transform of the chassis local origin.
    pub fn chassis_transform(&self) -> (Vec3, Quat) {
        (self.to_world(Vec3::ZERO), self.rotation)
    }

    pub fn steer_transform(&self) -> (Vec3, Quat) {
        (self.to_world(self.head), self.rotation * self.steer_frame())
    }

    pub fn fork_transform(&self) -> (Vec3, Quat) {
        let r = self.rotation * self.steer_frame();
        (self.to_world(self.head) + r * self.fork_slider_offset(), r)
    }

    pub fn swingarm_transform(&self) -> (Vec3, Quat) {
        (self.to_world(self.pivot), self.rotation * Quat::from_rotation_x(-self.swingarm_angle))
    }

    pub fn front_wheel_transform(&self) -> (Vec3, Quat) {
        let r = self.rotation * self.steer_frame();
        (self.to_world(self.front_axle_local()), r * Quat::from_rotation_x(-self.front.angle))
    }

    pub fn rear_wheel_transform(&self) -> (Vec3, Quat) {
        (
            self.to_world(self.rear_axle_local()),
            self.rotation * Quat::from_rotation_x(-self.swingarm_angle) * Quat::from_rotation_x(-self.rear.angle),
        )
    }

    /// Rear axle transform following swingarm motion but without wheel spin (for the sprocket carrier).
    pub fn rear_axle_transform(&self) -> (Vec3, Quat) {
        (
            self.to_world(self.rear_axle_local()),
            self.rotation * Quat::from_rotation_x(-self.swingarm_angle),
        )
    }

    /// World transform and scale for the drive chain, bridging the rear axle sprocket
    /// directly to the engine's front sprocket.
    pub fn chain_transform(&self) -> (Vec3, Quat, Vec3) {
        let ra_w = self.to_world(self.rear_axle_local());
        let bike_lat = self.rotation * Vec3::X;
        let sprocket_offset = self.def.geometry.rear_sprocket;
        let p_rear = ra_w + bike_lat * sprocket_offset;

        let fs_local = Vec3::new(sprocket_offset, self.front_sprocket.y, self.front_sprocket.z);
        let p_front = self.to_world(fs_local);

        let delta = p_front - p_rear;
        let dist = delta.length();
        let dir = delta / dist.max(1e-4);

        let up = bike_lat.cross(dir).normalize();
        let rot = Quat::from_mat3(&glam::Mat3::from_cols(-dir, bike_lat, up));
        let scale = Vec3::new(dist / 0.65, 1.0, 1.0);

        (p_rear, rot, scale)
    }

    pub fn on_ground(&self) -> bool {
        self.front.last_penetration > 0.0 || self.rear.last_penetration > 0.0
    }

    pub fn shock_compression(&self) -> f32 {
        self.linkage.shock_max - self.linkage.shock_length(self.swingarm_angle)
    }

    pub fn rider_view_world(&self) -> Vec3 {
        self.to_world(lh_to_rh(self.def.geometry.rider_view) + Vec3::Y * 0.55)
    }

    pub fn front_telemetry(&self) -> WheelTelemetry {
        self.front.telemetry
    }

    /// Front wheel spin rate (rad/s).
    pub fn front_omega(&self) -> f32 {
        self.front.omega
    }

    /// Rear wheel spin rate (rad/s).
    pub fn rear_omega(&self) -> f32 {
        self.rear.omega
    }

    pub fn rear_wheel_angle(&self) -> f32 {
        self.rear.angle
    }

    pub fn rear_telemetry(&self) -> WheelTelemetry {
        self.rear.telemetry
    }

    pub fn total_mass(&self) -> f32 {
        self.mass
    }

    pub fn is_linkage_solved(&self) -> bool {
        self.linkage.is_linkage_solved()
    }

    /// Mud stage shown on the bike and gear: the game cuts the dirt amount at 1/6, 1/2 and 5/6
    /// (FUN_1400523e0) and shows stage n with frame n of the mud paint layer.
    pub fn dirt_level(&self) -> u32 {
        dirt_level(self.dirt_factor)
    }

    pub fn fork_travel(&self) -> f32 {
        self.fork.travel
    }

    // ----- Simulation -----

    /// Advances by `dt` seconds using fixed 2 ms substeps (the game's 500 Hz physics rate).
    pub fn step<G: Ground + ?Sized>(&mut self, dt: f32, ground: &G) {
        if self.input.shift_up {
            self.powertrain.shift_up();
            self.input.shift_up = false;
        }
        if self.input.shift_down {
            self.powertrain.shift_down();
            self.input.shift_down = false;
        }
        self.accumulator += dt.min(0.1);
        while self.accumulator >= SUBSTEP {
            self.substep(SUBSTEP, ground);
            self.accumulator -= SUBSTEP;
        }
        let forward = self.rotation * Vec3::NEG_Z;
        self.speed_kmh = self.velocity.dot(forward) * 3.6;
        let right = self.rotation * Vec3::X;
        self.lean_angle_deg = (-right.y).asin().to_degrees();
    }

    fn evaluate_tyre(
        wheel: &mut Wheel,
        axle: Vec3,
        axle_vel: Vec3,
        spin_axis: Vec3,
        dt: f32,
        ground: &(impl Ground + ?Sized),
    ) -> Option<ContactResult> {
        let n = ground.normal(axle.x, axle.z);
        let r = wheel.tyre.radius();
        let rt = wheel.tyre.torus_radius().min(r * 0.5);
        let in_plane = -n + spin_axis * n.dot(spin_axis);
        let d = in_plane.normalize_or(-n);
        let tube = axle + d * (r - rt);
        let point = tube - n * rt;
        let gh = ground.height(point.x, point.z);
        let penetration = (gh - point.y) * n.y;
        if penetration <= 0.0 {
            wheel.last_penetration = 0.0;
            wheel.slip_long *= 0.9;
            wheel.slip_lat *= 0.9;
            wheel.telemetry = WheelTelemetry::default();
            return None;
        }
        let pen_rate = (penetration - wheel.last_penetration) / dt;
        wheel.last_penetration = penetration;
        let k = wheel.tyre.vertical_stiffness();
        let c = wheel.tyre.def.damper;
        let fz = (k * penetration + c * pen_rate).clamp(0.0, 40_000.0);

        let long = n.cross(spin_axis).normalize_or(Vec3::NEG_Z);
        let lat = n.cross(long);
        let contact_vel = axle_vel;
        let vx = contact_vel.dot(long);
        let vy = contact_vel.dot(lat);
        let r_eff = (axle - point).length().max(r * 0.5);
        let rolling = wheel.omega * r_eff;

        // First-order relaxation of the slip states (stable through standstill).
        let abs_vx = vx.abs();
        wheel.slip_long += ((rolling - vx) - abs_vx * wheel.slip_long) / RELAX_LONG * dt;
        wheel.slip_lat += ((-vy) - abs_vx * wheel.slip_lat) / RELAX_LAT * dt;
        wheel.slip_long = wheel.slip_long.clamp(-2.0, 2.0);
        wheel.slip_lat = wheel.slip_lat.clamp(-1.5, 1.5);
        let kappa = wheel.slip_long;
        let alpha = wheel.slip_lat.atan();
        let gamma = spin_axis.dot(n).clamp(-1.0, 1.0).asin();

        let surface = ground.surface(point.x, point.z);
        let grip_table = wheel.tyre.def.grip(surface);
        let grip = grip_table.dry_grip * wheel.grip_scale;
        let f = wheel.tyre.forces(fz, kappa, alpha, gamma, grip);
        let roll_res = wheel.tyre.rolling_resistance(fz, &grip_table, 0.0) * (vx / 0.5).clamp(-1.0, 1.0);

        // Low-speed damping keeps a parked bike from creeping on the relaxation springs.
        let creep = if abs_vx < 0.5 { (0.5 - abs_vx) * 2.0 } else { 0.0 };
        let fx = f.fx - roll_res - creep * vx * fz * 0.02;
        let fy = f.fy - creep * vy * fz * 0.05;

        wheel.telemetry = WheelTelemetry {
            load: fz,
            slip_ratio: kappa,
            slip_angle: alpha,
            camber: gamma,
            fx,
            fy,
            contact: Some(point),
            surface: Some(surface),
            rim_speed: wheel.omega * wheel.tyre.radius(),
            velocity: axle_vel,
            normal: n,
            rolling: long,
        };
        Some(ContactResult {
            force: long * fx + lat * fy + n * fz,
            point,
            aligning: n * f.mz,
            road_torque: -fx * r_eff,
        })
    }

    fn substep<G: Ground + ?Sized>(&mut self, dt: f32, ground: &G) {
        let inp = self.input;
        let rot = self.rotation;
        let g_vec = Vec3::new(0.0, -GRAVITY, 0.0);
        let speed = self.velocity.length();
        let forward = rot * Vec3::NEG_Z;
        let fwd_speed = self.velocity.dot(forward);

        // --- Front assembly kinematics.
        let steer_q = self.steer_frame();
        let axis_w = rot * self.steer_axis_local();
        let head_w = self.to_world(self.head);
        let fa_local = self.front_axle_local();
        let fa_w = self.to_world(fa_local);
        let fspin_w = rot * steer_q * Vec3::X;
        let fork_axis_w = axis_w;
        // Axle velocity: chassis motion + fork slide + steering rotation.
        let fa_vel = self.point_velocity(fa_w)
            + fork_axis_w * self.fork_speed
            + (-axis_w * self.steer_rate).cross(fa_w - head_w);

        // --- Rear assembly kinematics.
        let ra_local = self.rear_axle_local();
        let ra_w = self.to_world(ra_local);
        let pivot_w = self.to_world(self.pivot);
        let lat_w = rot * Vec3::X;
        let sw_omega_w = -lat_w * self.swingarm_rate;
        let ra_vel = self.point_velocity(ra_w) + sw_omega_w.cross(ra_w - pivot_w);
        let rspin_w = lat_w;

        // --- Drivetrain.
        let mut eff_throttle = inp.throttle;
        if self.traction_help {
            let slip_r = self.rear.telemetry.slip_ratio;
            if slip_r > 0.15 {
                let excess: f32 = (slip_r - 0.15).max(0.0);
                let cut: f32 = (1.0f32 - excess * 3.5f32).clamp(0.2f32, 1.0f32);
                eff_throttle *= cut;
            }
        }
        self.powertrain.throttle = eff_throttle;
        self.powertrain.clutch_lever = inp.clutch;
        self.powertrain.starter = inp.starter;
        self.powertrain.road_speed = fwd_speed;
        self.powertrain.wheel_radius = self.rear.tyre.radius();
        self.powertrain.rear_airborne = self.rear.telemetry.contact.is_none();
        let drive_torque = self.powertrain.step(dt, self.rear.omega);

        // --- Tyres.
        let fc = Self::evaluate_tyre(&mut self.front, fa_w, fa_vel, fspin_w, dt, ground);
        let rc = Self::evaluate_tyre(&mut self.rear, ra_w, ra_vel, rspin_w, dt, ground);

        // --- Rut deformation and dirt accumulation.
        // Dirt build-up as the game does it (FUN_1401a8480): while either wheel touches grass,
        // soil, soft soil or compact soil, dirt grows by 0.0001 per metre travelled. One amount
        // serves the bike and the rider gear.
        let dirty = |w: &Wheel| {
            w.telemetry.contact.is_some()
                && matches!(w.telemetry.surface, Some(Surface::Grass | Surface::Soil | Surface::SoftSoil | Surface::CompactSoil))
        };
        if dirty(&self.front) || dirty(&self.rear) {
            self.dirt_factor += dt * fwd_speed * 0.0001;
            self.rider_dirt_factor = self.dirt_factor;
        }
        // Ruts (FUN_1401a2fb0): contact point displacement between steps
        // v = length(pt_now - pt_prev) / dt (FUN_1401a2fb0 lines 700-705).
        // Ignored below 0.1 m/s (line 705).
        // The game dispatches physics at 500 Hz (dt = 0.002 s, FUN_140133e80 / FUN_1401be8a0).
        // We accumulate over the 500 Hz window (0.002 s) so the 0.1 mm threshold in ground.deform
        // is evaluated at the game rate.
        self.rut_accum += dt;
        if self.rut_accum >= 0.002 {
            let rut_dt = self.rut_accum;
            self.rut_accum = 0.0;
            for wheel in [&mut self.front, &mut self.rear] {
                let t = &wheel.telemetry;
                if let Some(pt) = t.contact {
                    let v = match wheel.last_contact {
                        Some(prev) => (pt - prev).length() / rut_dt,
                        None => (t.velocity - t.normal * t.velocity.dot(t.normal)).length(),
                    };
                    wheel.last_contact = Some(pt);
                    if v >= 0.1 {
                        let f = Vec3::new(t.fx, t.load, t.fy).length() + t.fx.abs();
                        ground.deform(pt.x, pt.z, v * f.min(5000.0) * 4e-5 * rut_dt);
                    }
                } else {
                    wheel.last_contact = None;
                }
            }
        }

        let mut force = g_vec * self.mass;
        let mut torque = Vec3::ZERO;

        // Rider dynamic body and weight shift (FUN_1401a8480).
        let lean_deg = (-(rot * Vec3::X).y).asin().to_degrees();
        let pitch_deg = (rot * Vec3::NEG_Z).y.clamp(-1.0, 1.0).asin().to_degrees();
        let fwd_accel = self.last_accel.dot(forward);
        self.rider_offset = self.rider.step(
            dt,
            &inp,
            fwd_speed,
            lean_deg,
            pitch_deg,
            fwd_accel,
            fc.is_none() && rc.is_none(),
        );
        let rider_w = self.to_world(self.rider_point + Vec3::new(self.rider_offset.x, self.rider.leg_pos, -self.rider_offset.y));
        let rider_ref_w = self.to_world(self.rider_point);
        torque += (rider_w - rider_ref_w).cross(g_vec * RIDER_MASS);

        // Stability help restoring torques (FUN_1401a8480 lines 2095-2275)
        if self.stability_help_scale > 0.0 {
            let roll_rate_deg = self.angular_velocity.dot(forward).to_degrees();
            let pitch_rate_deg = self.angular_velocity.dot(lat_w).to_degrees();
            let yaw_rate_deg = self.angular_velocity.dot(rot * Vec3::Y).to_degrees();
            let stab_local = self.rider.stability_torque(
                lean_deg,
                roll_rate_deg,
                pitch_deg,
                pitch_rate_deg,
                0.0,
                yaw_rate_deg,
                self.stability_help_scale,
            );
            torque += rot * stab_local;
        }

        // --- Front: fork slide, steering, wheel spin.
        let mut steer_torque = 0.0;
        let mut fork_ext = 0.0;
        let mut front_road_torque = 0.0;
        if let Some(c) = &fc {
            force += c.force;
            torque += (c.point - self.com).cross(c.force) + c.aligning;
            fork_ext += c.force.dot(fork_axis_w);
            // Torque about the steering axis (positive steer_angle = rotation about -axis).
            steer_torque -= axis_w.dot((c.point - head_w).cross(c.force) + c.aligning);
            front_road_torque = c.road_torque;
        }
        let a_chassis = self.last_accel;
        fork_ext += self.fork_mass * (g_vec - a_chassis).dot(fork_axis_w);
        let fork_force = self.fork.force(self.fork_compression, self.fork_speed);
        let fork_acc = (fork_ext - fork_force) / self.fork_mass;

        // Rider steering: MX Bikes' lean-target PID + yaw controller (steer_control.rs).
        let st = &self.def.steer;
        let right = rot * Vec3::X;
        let roll = (-right.y).asin();
        let roll_rate = self.angular_velocity.dot(forward);
        let v = fwd_speed.abs();
        let lock = st.lock_deg.to_radians();
        let front_load = fc.as_ref().map(|c| c.force.dot(fork_axis_w).max(0.0)).unwrap_or(0.0);
        let ground_n = ground.normal(self.com.x, self.com.z);
        let grip = self.front.tyre.def.coef("p_Dy1").max(0.3)
            * self.front.tyre.def.grip(ground.surface(self.com.x, self.com.z)).dry_grip
            * self.front.grip_scale;
        let ctrl_in = SteerInputs {
            dt,
            stick: inp.steer,
            speed: v,
            lean: roll.to_degrees(),
            heading: forward.x.atan2(-forward.z).to_degrees(),
            grip,
            bank: right.dot(ground_n).clamp(-1.0, 1.0).asin(),
            front_load,
            weight: self.mass * GRAVITY,
        };
        // Below 1 m/s the game switches from the lean PID to pointing the bars at Lock * stick with
        // gains derived from the steering inertia (Kp = -600 I, Kd = 0.05 Kp, FUN_140197060), i.e.
        // a bar acceleration of 600 * error - 30 * rate (FUN_14019d360).
        // The controller runs every step so its lean / yaw rate histories stay current.
        let pid = STEER_OUTPUT_SIGN * self.steer_gain_scale * self.steer_ctrl.torque(st, &ctrl_in);
        let mut rider_steer = if v >= 1.0 {
            pid
        } else {
            self.steer_inertia * (600.0 * (inp.steer * lock - self.steer_angle) - 30.0 * self.steer_rate)
        };
        if !self.rider_steering {
            rider_steer = 0.0;
        }
        let damper = -(st.k_damping[0] + st.k_damping[1] * v) * self.steer_rate;
        // Front wheel gyroscopic reaction about the steering axis.
        let lw = -fspin_w * self.front.spin_inertia * self.front.omega;
        let gyro_f = -(self.angular_velocity.cross(lw));
        steer_torque -= axis_w.dot(gyro_f);
        let steer_acc = (steer_torque + rider_steer + damper) / self.steer_inertia;

        // Feet down: below walking pace the rider props the bike up with a leg. STAND-IN: the
        // 2.5 m/s threshold, gains and target. The game's rider legs are part of its rider body
        // (rider.prm `Legs`); the 1.5..3 m/s ramp in FUN_1401a8480 belongs to the stand command
        // (rider_state.rs), not to the feet.
        let foot = if v < 2.5 { 1.0 } else { 0.0 };
        if foot > 0.0 && !self.crashed {
            let target = inp.steer * 0.15;
            let support = -(2200.0 * (roll - target) + 400.0 * roll_rate) * foot;
            torque += forward * support;
        }

        // --- Rear: swingarm about the pivot driven by tyre, shock and chain.
        let mut sw_torque = 0.0;
        let mut rear_road_torque = 0.0;
        if let Some(c) = &rc {
            force += c.force;
            torque += (c.point - self.com).cross(c.force) + c.aligning;
            sw_torque += (-lat_w).dot((c.point - pivot_w).cross(c.force));
            rear_road_torque = c.road_torque;
        }
        let sw_cg = pivot_w + (ra_w - pivot_w) * 0.7;
        sw_torque += (-lat_w).dot((sw_cg - pivot_w).cross((g_vec - a_chassis) * self.swingarm_mass));
        let stroke = self.linkage.shock_max - self.linkage.shock_min;
        let compression = self.shock_compression();
        let rate = self.linkage.shock_rate(self.swingarm_angle);
        let shock_speed = -rate * self.swingarm_rate;
        let shock_force = self.shock.force(compression, stroke, shock_speed);
        sw_torque += shock_force * rate;
        // Chain pull: drive torque over the rear sprocket radius along the chain line.
        let rs_radius = (self.def.driveline.rear_sprocket as f32 * self.def.geometry.chain_pitch / std::f32::consts::TAU).max(0.05);
        let chain_force = drive_torque.max(0.0) / rs_radius;
        let fs_w = self.to_world(self.front_sprocket);
        let rs_top_w = ra_w + rot * Vec3::Y * rs_radius;
        let chain_dir = (fs_w - rs_top_w).normalize_or_zero();
        sw_torque += (-lat_w).dot((rs_top_w - pivot_w).cross(chain_dir * chain_force));
        let sw_acc = sw_torque / self.swingarm_inertia;

        // --- Wheel spin.
        let mut front_lever = inp.front_brake;
        let rear_lever = inp.rear_brake;
        if self.brake_help {
            // Anti-loop stoppie modulation: if rear wheel load is lost and pitching forward
            let rear_load = self.rear.telemetry.load;
            let pitch_rate = self.angular_velocity.dot(lat_w);
            if rear_load < 100.0 && pitch_rate < 0.0 {
                let fade = (rear_load / 100.0).clamp(0.1, 1.0);
                front_lever *= fade;
            }
            // Anti-lock front: avoid wheel locking up at forward speed
            let r_f = self.front.tyre.radius();
            let v_front = fwd_speed.max(0.0);
            if v_front > 3.0 && self.front.omega < (0.15 * v_front / r_f) {
                front_lever *= 0.5;
            }
        }
        let brake = |w: &Wheel, lever: f32, extra: f32| -> f32 { w.brake_torque_max * lever.clamp(0.0, 1.0) + extra };
        let fb = brake(&self.front, front_lever, 0.5);
        let rb = brake(&self.rear, rear_lever, 0.5);
        let spin_before = (self.front.omega, self.rear.omega);
        for (w, drive, road, b) in [
            (&mut self.front, 0.0, front_road_torque, fb),
            (&mut self.rear, drive_torque, rear_road_torque, rb),
        ] {
            let free = drive + road;
            w.omega += free / w.spin_inertia * dt;
            let stop = b * dt / w.spin_inertia;
            if w.omega.abs() <= stop {
                w.omega = 0.0;
            } else {
                w.omega -= stop * w.omega.signum();
            }
            w.angle = (w.angle + w.omega * dt) % std::f32::consts::TAU;
        }

        // --- Aerodynamics.
        let cda = self.def.aero.max_cda;
        let drag = -0.5 * AIR_DENSITY * cda * speed * self.velocity;
        let lift: Vec3 = (rot * Vec3::Y) * (0.5 * AIR_DENSITY * self.def.aero.cl * self.def.aero.area.z * fwd_speed * fwd_speed);
        let aero_w = self.to_world(self.aero_point);
        force += drag + lift;
        torque += (aero_w - self.com).cross(drag + lift);
        torque -= self.angular_velocity * self.angular_velocity.length() * 0.5;

        // --- Body contacts (crash spheres from the chassis collision volumes and the rider).
        let mut max_contact: Option<BodyContact> = None;
        for (local, radius) in self.body_spheres() {
            let p = self.to_world(local);
            let gh = ground.height(p.x, p.z);
            let depth = gh + radius - p.y;
            if depth > 0.0 {
                let n = ground.normal(p.x, p.z);
                let vel = self.point_velocity(p);
                let vn = vel.dot(n);
                let fn_ = (60_000.0 * depth - 3_000.0 * vn).max(0.0);
                let vt = vel - n * vn;
                let ft = -vt.normalize_or_zero() * (fn_ * 0.6).min(vt.length() * self.mass / dt * 0.2);
                let f = n * fn_ + ft;
                force += f;
                torque += (p - n * radius - self.com).cross(f);
                if max_contact.as_ref().map_or(true, |c| fn_ > c.normal_force) {
                    max_contact = Some(BodyContact {
                        point: p,
                        normal: n,
                        normal_force: fn_,
                        scrape_speed: vt.length(),
                        impact_impulse: fn_ * dt,
                    });
                }
            }
        }
        self.last_body_contact = max_contact;

        // --- Internal DoF reactions on the chassis.
        let fork_rel = fork_axis_w * (fork_acc * self.fork_mass);
        force -= fork_rel;
        torque -= (fa_w - self.com).cross(fork_rel);
        torque -= -axis_w * steer_acc * self.steer_inertia;
        torque -= -lat_w * sw_acc * self.swingarm_inertia;
        let lw_r = -rspin_w * self.rear.spin_inertia * self.rear.omega;
        let l_spin = lw + lw_r;
        // Gyroscopic coupling of the spinning wheels (steering axis share already applied).
        let gyro_chassis = -(self.angular_velocity.cross(l_spin)) - axis_w * axis_w.dot(gyro_f);
        torque += gyro_chassis;
        // Spin angular momentum exchanged with the wheels (engine / brake reactions).
        let dwf = (self.front.omega - spin_before.0) / dt;
        let dwr = (self.rear.omega - spin_before.1) / dt;
        torque -= -fspin_w * self.front.spin_inertia * dwf - rspin_w * self.rear.spin_inertia * dwr;

        // --- Integrate chassis.
        let accel = force / self.mass;
        self.last_accel = accel;
        self.velocity += accel * dt;
        self.com += self.velocity * dt;
        let r = Mat3::from_quat(rot);
        let i_world = r * self.inertia_local * r.transpose();
        let i_inv = r * self.inertia_local_inv * r.transpose();
        let w = self.angular_velocity;
        let alpha = i_inv * (torque - w.cross(i_world * w));
        self.angular_velocity += alpha * dt;
        let dq = Quat::from_scaled_axis(self.angular_velocity * dt);
        self.rotation = (dq * self.rotation).normalize();

        // --- Integrate internal DoFs.
        self.fork_speed += fork_acc * dt;
        self.fork_compression += self.fork_speed * dt;
        if self.fork_compression < -0.002 {
            self.fork_compression = -0.002;
            self.fork_speed = self.fork_speed.max(0.0);
        }
        if self.fork_compression > self.fork.travel {
            self.fork_compression = self.fork.travel;
            self.fork_speed = self.fork_speed.min(0.0);
        }

        self.steer_rate += steer_acc * dt;
        self.steer_angle += self.steer_rate * dt;
        let lock = self.def.steer.lock_deg.to_radians();
        if self.steer_angle.abs() > lock {
            self.steer_angle = self.steer_angle.clamp(-lock, lock);
            self.steer_rate = 0.0;
        }

        self.swingarm_rate += sw_acc * dt;
        self.swingarm_angle += self.swingarm_rate * dt;
        let (lo, hi) = self.linkage.angle_range;
        if self.swingarm_angle < lo {
            self.swingarm_angle = lo;
            self.swingarm_rate = self.swingarm_rate.max(0.0);
        }
        if self.swingarm_angle > hi + 0.02 {
            self.swingarm_angle = hi + 0.02;
            self.swingarm_rate = self.swingarm_rate.min(0.0);
        }

        // Rider detachment (FUN_1401a8480, FUN_1401a79f0):
        // 1. Contact sphere: 0.31 m sphere 0.44 m above rider body point touches ground.
        let fall_world = self.to_world(self.rider_point + Vec3::new(0.0, 0.44 + self.rider.leg_pos, 0.0));
        if fall_world.y - ground.height(fall_world.x, fall_world.z) - 0.31 <= 0.0 {
            self.crashed = true;
        }
        // 2. NOT PORTED: the game also detaches the rider when the 25-step average of the forces
        // in the rider body's attachment springs passes 4000 N lateral / 10000 N longitudinal
        // (15000 N / -2000 N vertical on some bikes) (FUN_1401a8480, the block before the pitch
        // spring update). Those springs belong to the rider body simulation, which is not ported;
        // applying the limits to the whole chassis force (as a previous session did) crashed the
        // bike on every hard landing or slide.
    }

    /// Collision spheres (chassis local): `.geom` chassis spheres plus rider head and bar ends.
    fn body_spheres(&self) -> [(Vec3, f32); 6] {
        let bars = self.head + self.steer_frame() * Vec3::new(0.0, 0.25, 0.0);
        [
            (lh_to_rh(Vec3::new(0.0, 0.8, 0.4)), 0.2),
            (lh_to_rh(Vec3::new(0.0, 0.8, -0.65)), 0.18),
            (lh_to_rh(Vec3::new(0.0, 0.43, -0.02)), 0.12),
            (lh_to_rh(self.def.geometry.rider_mass) + Vec3::Y * 0.55, 0.15),
            (bars + Vec3::X * 0.39, 0.06),
            (bars - Vec3::X * 0.39, 0.06),
        ]
    }
}
