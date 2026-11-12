//! Antenna description: wires, sources, loads, ground and options.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wire {
    pub p1: [f64; 3],
    pub p2: [f64; 3],
    /// Wire radius in metres.
    pub r: f64,
    /// Segment count; 0 or negative means automatic segmentation.
    pub seg: i32,
    /// Insulation thickness in metres (0 = bare wire).
    pub ins_t: f64,
    /// Relative permittivity of the insulation (PVC about 3.5, PE 2.3, PTFE 2.1).
    pub ins_eps: f64,
}

/// Common insulation materials and their relative permittivity.
pub const INSULATIONS: [(&str, f64); 5] = [("PVC", 3.5), ("Polyethylene", 2.3), ("PTFE (Teflon)", 2.1), ("Rubber", 3.0), ("Enamel", 3.6)];

impl Wire {
    pub fn new(p1: [f64; 3], p2: [f64; 3], r: f64) -> Self {
        Wire { p1, p2, r, seg: -1, ins_t: 0.0, ins_eps: 1.0 }
    }
    pub fn insulated(&self) -> bool {
        self.ins_t > 0.0 && self.ins_eps > 1.0 && self.r.abs() > 0.0
    }
    /// Extra series inductance per metre that a dielectric sleeve adds (H/m):
    /// L' = μ0/(2π) · (1 − 1/εr) · ln((a + t)/a)  — the standard thin-insulation model
    /// (Tsai; also what NEC-4's IS card and common "insulated wire" corrections use).
    pub fn insulation_l_per_m(&self) -> f64 {
        if !self.insulated() {
            return 0.0;
        }
        let a = self.r.abs();
        2.0e-7 * (1.0 - 1.0 / self.ins_eps) * ((a + self.ins_t) / a).ln()
    }
    /// Rough velocity factor of this wire as a resonant element, VF = 1/sqrt(1 + L'/L0),
    /// with L0 = μ0/(2π)·ln(L/a) the wire's own inductance per metre (an estimate for display).
    pub fn velocity_factor(&self) -> f64 {
        let lp = self.insulation_l_per_m();
        if lp <= 0.0 {
            return 1.0;
        }
        let l0 = 2.0e-7 * (self.length().max(10.0 * self.r.abs()) / self.r.abs()).ln().max(1.0);
        1.0 / (1.0 + lp / l0).sqrt()
    }
    pub fn length(&self) -> f64 {
        let d = [self.p2[0] - self.p1[0], self.p2[1] - self.p1[1], self.p2[2] - self.p1[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Source {
    pub wire: usize,
    /// Position along the wire, 0 = begin, 0.5 = centre, 1 = end.
    pub pos: f64,
    pub phase_deg: f64,
    pub volts: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LoadKind {
    /// Fixed impedance R + jX (ohms).
    Rx { r: f64, x: f64 },
    /// Inductor (uH) and/or capacitor (pF) with quality factor Q (0 = lossless).
    /// Only L or only C: a series reactance; both: a parallel L-C trap (Q applies to L).
    /// (FFSolver 0x408200 / 0x407460, see MECHANICS.md §3)
    Lc { l_uh: f64, c_pf: f64, q: f64 },
    /// NEC-style lumped R, L, C in series (LD 0) or parallel (LD 1); a zero value means that
    /// part is absent.
    Rlc { r: f64, l_uh: f64, c_pf: f64, parallel: bool },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Load {
    pub wire: usize,
    pub pos: f64,
    pub kind: LoadKind,
}

impl Load {
    pub fn impedance(&self, freq_hz: f64) -> num_complex::Complex64 {
        use num_complex::Complex64 as C;
        // The original works with f in MHz, L in uH and C in pF (FFSolver 0x408200 / 0x407460).
        const TWO_PI: f64 = 6.283185307179586; // DAT_00425b10
        const TWO_PI_E6: f64 = 6.283185307179586e-06; // DAT_00425b08
        let f_mhz = freq_hz * 1e-6;
        match self.kind {
            LoadKind::Rx { r, x } => C::new(r, x),
            LoadKind::Lc { l_uh, c_pf, q } => {
                let xl = f_mhz * l_uh * TWO_PI;
                if l_uh != 0.0 && c_pf != 0.0 {
                    let rl = if q != 0.0 { xl / q } else { 0.0 };
                    let y = 1.0 / C::new(rl, xl) + C::new(0.0, f_mhz * c_pf * TWO_PI_E6);
                    if y.norm() == 0.0 { C::new(1e20, 0.0) } else { 1.0 / y }
                } else if l_uh != 0.0 {
                    C::new(if q != 0.0 { xl / q } else { 0.0 }, xl)
                } else if c_pf != 0.0 {
                    let xc = -1.0 / (f_mhz * c_pf * TWO_PI_E6);
                    C::new(if q != 0.0 { -xc / q } else { 0.0 }, xc)
                } else {
                    C::new(0.0, 0.0)
                }
            }
            LoadKind::Rlc { r, l_uh, c_pf, parallel } => {
                let w = TWO_PI * freq_hz;
                if parallel {
                    let mut y = C::new(0.0, 0.0);
                    if r != 0.0 {
                        y += 1.0 / r;
                    }
                    if l_uh != 0.0 {
                        y += 1.0 / C::new(0.0, w * l_uh * 1e-6);
                    }
                    if c_pf != 0.0 {
                        y += C::new(0.0, w * c_pf * 1e-12);
                    }
                    if y.norm() == 0.0 { C::new(1e20, 0.0) } else { 1.0 / y }
                } else {
                    let mut z = C::new(r, 0.0);
                    if l_uh != 0.0 {
                        z += C::new(0.0, w * l_uh * 1e-6);
                    }
                    if c_pf != 0.0 {
                        z += C::new(0.0, -1.0 / (w * c_pf * 1e-12));
                    }
                    z
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroundKind {
    Free,
    Perfect,
    Real,
}

impl GroundKind {
    pub fn label(&self) -> &'static str {
        match self {
            GroundKind::Free => "Free space",
            GroundKind::Perfect => "Perfect ground",
            GroundKind::Real => "Real ground",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ground {
    pub kind: GroundKind,
    /// Relative permittivity of real ground.
    pub eps_r: f64,
    /// Conductivity of real ground in mS/m.
    pub sigma_ms: f64,
    /// Extra height added to every wire when a ground is used (m).
    pub add_height: f64,
    /// A second ground medium (sea, marsh) away from the antenna, far field only.
    pub second: SecondMedium,
}

/// A second ground medium beyond a straight boundary or outside a circle, as
/// NEC-2's GD card: it changes the ground reflection in the far field, not the
/// currents.  Positions are relative to the origin (before "add height").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SecondMedium {
    pub enabled: bool,
    pub eps_r: f64,
    /// mS/m
    pub sigma_ms: f64,
    /// boundary is a circle about the origin (medium 2 outside) instead of a line
    pub circle: bool,
    /// distance from the origin to the line, or the circle's radius (m)
    pub distance: f64,
    /// direction from the origin square to the line, degrees from +X toward +Y
    /// (azimuth, as the patterns): medium 2 lies that way
    pub bearing_deg: f64,
    /// how far medium 2's surface lies below the primary ground (m)
    pub drop: f64,
    /// smooth transition over the first Fresnel zone (wiremom, MININEC) instead of
    /// the medium at the reflection point (NEC-2)
    pub fresnel: bool,
}

impl Default for SecondMedium {
    fn default() -> Self {
        SecondMedium { enabled: false, eps_r: 81.0, sigma_ms: 5000.0, circle: false, distance: 30.0, bearing_deg: 0.0, drop: 0.0, fresnel: true }
    }
}

impl SecondMedium {
    /// The wiremom description (None unless enabled).
    pub fn to_wiremom(&self) -> Option<wiremom::SecondGround> {
        if !self.enabled {
            return None;
        }
        let boundary = if self.circle {
            wiremom::Boundary::Circle { radius: self.distance.max(0.0) }
        } else {
            wiremom::Boundary::Line { distance: self.distance, azimuth: self.bearing_deg.to_radians() }
        };
        let mut g = wiremom::SecondGround::new(wiremom::RealGround { eps_r: self.eps_r, sigma: self.sigma_ms * 1e-3 }, boundary);
        g.drop = self.drop.max(0.0);
        g.fresnel = self.fresnel;
        Some(g)
    }

    /// One-line form stored in MMANA comments and NEC CM cards.
    pub fn encode(&self) -> String {
        format!(
            "wireant ground2 {} {} {} {} {} {} {}",
            self.eps_r,
            self.sigma_ms,
            if self.circle { "circle" } else { "line" },
            self.distance,
            self.bearing_deg,
            self.drop,
            if self.fresnel { "fresnel" } else { "specular" }
        )
    }

    pub fn decode(line: &str) -> Option<SecondMedium> {
        let rest = line.trim().strip_prefix("wireant ground2")?;
        let t: Vec<&str> = rest.split_whitespace().collect();
        if t.len() < 7 {
            return None;
        }
        let f = |i: usize| t[i].parse::<f64>().ok();
        Some(SecondMedium {
            enabled: true,
            eps_r: f(0)?,
            sigma_ms: f(1)?,
            circle: t[2] == "circle",
            distance: f(3)?,
            bearing_deg: f(4)?,
            drop: f(5)?,
            fresnel: t[6] != "specular",
        })
    }
}

/// Typical ground constants (εr, σ in mS/m), as in NEC/ITU tables.
pub const GROUND_PRESETS: [(&str, f64, f64); 7] = [
    ("Sea water", 81.0, 5000.0),
    ("Fresh water", 80.0, 1.0),
    ("Very good (pastoral, rich soil)", 20.0, 30.0),
    ("Average", 13.0, 5.0),
    ("Poor (rocky, sandy)", 5.0, 1.0),
    ("Very poor (city, industrial)", 3.0, 1.0),
    ("Wet ground (ITU)", 30.0, 10.0),
];

/// Which solver calculates the antenna.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// The built-in multi-threaded MININEC-style solver.
    Mininec,
    /// The bundled public-domain NEC-2 engine (nec2c), with Sommerfeld real ground.
    Nec2,
    /// The wiremom engine: MPIE with triangle basis functions and Galerkin testing,
    /// Sommerfeld real ground, in-process and multi-threaded.
    Wiremom,
    /// An external NEC program the user has a licence for (NEC-4, NEC-5 or another
    /// build), set under File → Preferences; run like NEC-2 with the same card deck.
    External,
}

impl Engine {
    pub fn label(&self) -> &'static str {
        match self {
            Engine::Mininec => "MININEC (built-in, multi-threaded)",
            Engine::Nec2 => "NEC-2 (nec2c, Sommerfeld ground)",
            Engine::Wiremom => "wiremom (built-in MPIE, Sommerfeld ground)",
            Engine::External => "External NEC (your own copy; experimental)",
        }
    }
}

/// Wire materials in the original's order; the index is the code in the .maa file
/// (language file english.mmn: WireSel0..WireSel8). Conductivity and permeability values are
/// estimates (the protected main program holds the real ones); "pipe" uses the same skin-effect
/// loss as "wire".
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    Lossless,
    CuWire,
    CuPipe,
    AlWire,
    AlPipe,
    FeWire,
    FePipe,
    UserWire,
    UserPipe,
}

impl Material {
    pub const ALL: [Material; 9] = [
        Material::Lossless,
        Material::CuWire,
        Material::CuPipe,
        Material::AlWire,
        Material::AlPipe,
        Material::FeWire,
        Material::FePipe,
        Material::UserWire,
        Material::UserPipe,
    ];
    pub fn label(&self) -> &'static str {
        match self {
            Material::Lossless => "No loss",
            Material::CuWire => "Cu wire",
            Material::CuPipe => "Cu pipe",
            Material::AlWire => "Al wire",
            Material::AlPipe => "Al pipe",
            Material::FeWire => "Fe wire",
            Material::FePipe => "Fe pipe",
            Material::UserWire => "User wire",
            Material::UserPipe => "User pipe",
        }
    }
    /// (conductivity S/m, relative permeability); None = perfect conductor.
    pub fn conduction(&self, user_rho: f64, user_mu: f64) -> Option<(f64, f64)> {
        match self {
            Material::Lossless => None,
            Material::CuWire | Material::CuPipe => Some((5.8e7, 1.0)),
            Material::AlWire | Material::AlPipe => Some((3.5e7, 1.0)),
            Material::FeWire | Material::FePipe => Some((1.0e7, 100.0)),
            Material::UserWire | Material::UserPipe => {
                if user_rho > 0.0 { Some((1.0 / user_rho, user_mu.max(1.0))) } else { None }
            }
        }
    }
    pub fn is_user(&self) -> bool {
        matches!(self, Material::UserWire | Material::UserPipe)
    }
    pub fn code(&self) -> i32 {
        Material::ALL.iter().position(|m| m == self).unwrap_or(0) as i32
    }
    pub fn from_code(c: i32) -> Material {
        *Material::ALL.get(c.max(0) as usize).unwrap_or(&Material::Lossless)
    }
}

/// Automatic segmentation: shortest segment lambda/dm1 at the wire ends, longest lambda/dm2,
/// growing by factor `sc` from the ends; `ec` = number of shortest segments at each end
/// (FFSolver 0x405540, see MECHANICS.md §2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Segmentation {
    pub dm1: f64,
    pub dm2: f64,
    pub sc: f64,
    pub ec: i32,
}

impl Default for Segmentation {
    fn default() -> Self {
        Segmentation { dm1: 400.0, dm2: 40.0, sc: 2.0, ec: 1 }
    }
}

/// A flat conducting plate (parallelogram) from `corner` along `edge1` and
/// `edge2`.  Modelled by the wiremom engine; a wire ending on the plate connects to
/// it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Plate {
    pub corner: [f64; 3],
    pub edge1: [f64; 3],
    pub edge2: [f64; 3],
    /// Mesh divisions along each edge; 0 = automatic (about a tenth of a wavelength).
    pub n1: usize,
    pub n2: usize,
}

impl Plate {
    pub fn new(corner: [f64; 3], edge1: [f64; 3], edge2: [f64; 3]) -> Plate {
        Plate { corner, edge1, edge2, n1: 0, n2: 0 }
    }
    pub fn corners(&self) -> [[f64; 3]; 4] {
        let p = |a: f64, b: f64| [0, 1, 2].map(|i| self.corner[i] + a * self.edge1[i] + b * self.edge2[i]);
        [p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)]
    }
}

/// A transmission line between two places on wires (a feed line, phasing line or
/// stub).  Modelled by the wiremom and NEC-2 engines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TLine {
    pub a_wire: usize,
    pub a_pos: f64,
    pub b_wire: usize,
    pub b_pos: f64,
    /// Characteristic impedance (ohms).
    pub z0: f64,
    /// Length in metres; 0 = the straight distance between the two ends.
    pub length: f64,
    /// Velocity factor (1 = air line, about 0.66 for solid-PE coax).
    pub vf: f64,
    /// Loss in dB per 100 m at the calculation frequency.
    pub loss_db: f64,
    /// Conductors swapped at the far end (a half twist; NEC's negative Z0).
    pub reversed: bool,
}

impl TLine {
    pub fn new(a_wire: usize, a_pos: f64, b_wire: usize, b_pos: f64) -> TLine {
        TLine { a_wire, a_pos, b_wire, b_pos, z0: 50.0, length: 0.0, vf: 1.0, loss_db: 0.0, reversed: false }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Antenna {
    pub name: String,
    pub freq_mhz: f64,
    pub wires: Vec<Wire>,
    pub sources: Vec<Source>,
    pub loads: Vec<Load>,
    /// Transmission lines (not stored in .maa files).
    pub lines: Vec<TLine>,
    /// Conducting plates (not stored in .maa files).
    pub plates: Vec<Plate>,
    pub ground: Ground,
    pub material: Material,
    /// Reference impedance for SWR (ohms).
    pub z0: f64,
    pub seg: Segmentation,
    pub comment: String,
    /// Rear range for F/B (degrees): azimuth width around the back and vertical range
    /// (the "AzEl" fields of the G/H/M/R/AzEl/X line, 120 and 60 in most example files).
    pub fb_az: f64,
    pub fb_el: f64,
    /// User material: resistivity (ohm·m) and relative permeability.
    pub user_rho: f64,
    pub user_mu: f64,
    /// Solver choice (an app setting, not stored in .maa files).
    pub engine: Engine,
}

pub fn pos_label(wire: usize, pos: f64) -> String {
    if pos <= 1e-9 {
        format!("w{}b", wire + 1)
    } else if (pos - 0.5).abs() < 1e-9 {
        format!("w{}c", wire + 1)
    } else if pos >= 1.0 - 1e-9 {
        format!("w{}e", wire + 1)
    } else {
        format!("w{}p{:.1}", wire + 1, pos * 100.0)
    }
}

impl Antenna {
    pub fn lambda(&self) -> f64 {
        299.792458 / self.freq_mhz
    }

    pub fn example_names() -> &'static [&'static str] {
        &[
            "Half-wave dipole (14 MHz)",
            "Inverted V (7 MHz)",
            "3-element Yagi (14 MHz)",
            "Quarter-wave vertical (7 MHz)",
            "Ground plane with 4 radials (28 MHz)",
            "Two-element quad (21 MHz)",
        ]
    }

    /// A new, empty antenna: no wires, sources or loads; free space, copper wire,
    /// 14.15 MHz until the user sets the frequency.
    pub fn blank() -> Antenna {
        let mut a = Antenna::example(0);
        a.name = "New antenna".into();
        a.wires.clear();
        a.sources.clear();
        a.loads.clear();
        a.lines.clear();
        a.plates.clear();
        a.comment.clear();
        a.ground = Ground { kind: GroundKind::Free, eps_r: 13.0, sigma_ms: 5.0, add_height: 0.0, second: SecondMedium::default() };
        a.material = Material::CuWire;
        a
    }

    pub fn example(i: usize) -> Antenna {
        let real = |h: f64| Ground { kind: GroundKind::Real, eps_r: 13.0, sigma_ms: 5.0, add_height: h, second: SecondMedium::default() };
        let base = |name: &str, f: f64| Antenna {
            name: name.to_string(),
            freq_mhz: f,
            wires: vec![],
            sources: vec![],
            loads: vec![],
            lines: vec![],
            plates: vec![],
            ground: Ground { kind: GroundKind::Free, eps_r: 13.0, sigma_ms: 5.0, add_height: 0.0, second: SecondMedium::default() },
            material: Material::CuWire,
            z0: 50.0,
            seg: Segmentation::default(),
            comment: String::new(),
            fb_az: 120.0,
            fb_el: 60.0,
            user_rho: 1.72e-8,
            user_mu: 1.0,
            engine: Engine::Wiremom,
        };
        let src = |w| Source { wire: w, pos: 0.5, phase_deg: 0.0, volts: 1.0 };
        match i {
            1 => {
                let mut a = base("Inverted V for 7.1 MHz", 7.1);
                a.wires.push(Wire::new([0.0, 0.0, 12.0], [0.0, -9.0, 7.5], 1.0e-3));
                a.wires.push(Wire::new([0.0, 0.0, 12.0], [0.0, 9.0, 7.5], 1.0e-3));
                a.sources.push(Source { wire: 0, pos: 0.0, phase_deg: 0.0, volts: 1.0 });
                a.ground = real(0.0);
                a
            }
            2 => {
                let mut a = base("3-element Yagi for 14.15 MHz", 14.15);
                let r = 12.5e-3;
                a.wires.push(Wire::new([-4.2, -5.42, 0.0], [-4.2, 5.42, 0.0], r));
                a.wires.push(Wire::new([0.0, -5.08, 0.0], [0.0, 5.08, 0.0], r));
                a.wires.push(Wire::new([3.2, -4.86, 0.0], [3.2, 4.86, 0.0], r));
                a.sources.push(src(1));
                a.ground = real(15.0);
                a.material = Material::AlPipe;
                a
            }
            3 => {
                let mut a = base("Quarter-wave vertical for 7.1 MHz", 7.1);
                a.wires.push(Wire::new([0.0, 0.0, 0.0], [0.0, 0.0, 10.2], 5.0e-3));
                a.sources.push(Source { wire: 0, pos: 0.0, phase_deg: 0.0, volts: 1.0 });
                a.ground = Ground { kind: GroundKind::Perfect, ..real(0.0) };
                a
            }
            4 => {
                let mut a = base("Ground plane for 28.5 MHz", 28.5);
                let r = 2.0e-3;
                a.wires.push(Wire::new([0.0, 0.0, 0.0], [0.0, 0.0, 2.55], r));
                let d = 2.6 * std::f64::consts::FRAC_1_SQRT_2;
                let drop = -1.2;
                for (x, y) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)] {
                    let _ = d;
                    a.wires.push(Wire::new([0.0, 0.0, 0.0], [x * 2.35, y * 2.35, drop], r));
                }
                a.sources.push(Source { wire: 0, pos: 0.0, phase_deg: 0.0, volts: 1.0 });
                a
            }
            5 => {
                let mut a = base("Two-element quad for 21.2 MHz", 21.2);
                let r = 1.0e-3;
                let sq = |x: f64, s: f64, a: &mut Antenna| {
                    let p = [[x, -s, -s], [x, s, -s], [x, s, s], [x, -s, s]];
                    for k in 0..4 {
                        a.wires.push(Wire::new(p[k], p[(k + 1) % 4], r));
                    }
                };
                sq(0.0, 1.80, &mut a);
                sq(-2.8, 1.88, &mut a);
                a.sources.push(src(0));
                a
            }
            _ => {
                let mut a = base("Half-wave dipole for 14.15 MHz", 14.15);
                a.wires.push(Wire::new([0.0, -5.14, 0.0], [0.0, 5.14, 0.0], 1.0e-3));
                a.sources.push(src(0));
                a
            }
        }
    }
}

// Defaults let JSON (the MCP interface) leave fields out.
impl Default for Wire {
    fn default() -> Self {
        Wire::new([0.0; 3], [0.0; 3], 1.0e-3)
    }
}
impl Default for Source {
    fn default() -> Self {
        Source { wire: 0, pos: 0.5, phase_deg: 0.0, volts: 1.0 }
    }
}
impl Default for Load {
    fn default() -> Self {
        Load { wire: 0, pos: 0.5, kind: LoadKind::Rx { r: 0.0, x: 0.0 } }
    }
}
impl Default for Ground {
    fn default() -> Self {
        Antenna::blank().ground
    }
}
impl Default for Plate {
    fn default() -> Self {
        Plate::new([0.0; 3], [0.0; 3], [0.0; 3])
    }
}
impl Default for TLine {
    fn default() -> Self {
        TLine::new(0, 0.5, 0, 0.5)
    }
}
impl Default for Antenna {
    fn default() -> Self {
        Antenna::blank()
    }
}
