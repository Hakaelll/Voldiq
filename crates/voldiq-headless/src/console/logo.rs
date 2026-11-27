use super::terminal::Terminal;
use std::io::{self, Write};

const DENSITY: &[u8] = b" .:-=+*#%@";
pub(super) const REST_YAW: f64 = -0.20;
const LEAN: f64 = 0.19;
const TOP: f64 = 0.54;
const BASE: f64 = -0.67;
const DISK_TOPS: [f64; 3] = [TOP, 0.13, -0.28];
const DISK_HEIGHT: f64 = 0.39;
const RIM_WIDTH: f64 = 0.09;

#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub(super) menu_column: usize,
    pub(super) columns: usize,
    pub(super) rows: usize,
    #[cfg(windows)]
    pub(super) terminal_width: usize,
    #[cfg(windows)]
    pub(super) rows_above_prompt: Option<usize>,
}

pub(super) struct Cell {
    pub(super) glyph: u8,
    pub(super) ink: Ink,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Ink {
    Neutral,
    Shadow,
    Sector,
}

pub(super) fn write(
    out: &mut impl Write,
    terminal: &Terminal,
    menu: &[String],
    menu_width: usize,
) -> io::Result<Layout> {
    let terminal_width = terminal.width();
    let beside_columns = terminal_width.saturating_sub(menu_width + 6);
    let beside_menu = beside_columns >= 20;
    let available = if beside_menu {
        beside_columns
    } else {
        terminal_width.saturating_sub(3)
    };
    let columns = available.clamp(1, 56);
    let rows = (columns * 21).div_ceil(56);
    let panel = if beside_menu { menu } else { &[] };
    let frame_rows = rows.max(panel.len());
    let top = (frame_rows - rows) / 2;
    let layout = Layout {
        menu_column: if beside_menu { columns + 5 } else { 2 },
        columns,
        rows,
        #[cfg(windows)]
        terminal_width,
        #[cfg(windows)]
        rows_above_prompt: (terminal_width > menu_width + 2)
            .then_some(frame_rows + 1 + if beside_menu { 0 } else { menu.len() } - top),
    };
    let mut cells = layout.cells(REST_YAW);
    let mut frame = String::with_capacity((columns + 4) * frame_rows);
    let panel_top = (frame_rows - panel.len()) / 2;
    for row in 0..frame_rows {
        frame.push_str("  ");
        if (top..top + rows).contains(&row) {
            write_row(&mut frame, terminal, cells.by_ref().take(columns));
        } else {
            frame.extend(std::iter::repeat_n(' ', columns));
        }
        if let Some(line) = row
            .checked_sub(panel_top)
            .and_then(|index| panel.get(index))
        {
            frame.push_str("   ");
            frame.push_str(line);
        }
        frame.push('\n');
    }
    out.write_all(frame.as_bytes())?;
    if !beside_menu {
        for line in menu {
            writeln!(out, "  {line}")?;
        }
    }
    Ok(layout)
}

impl Layout {
    pub(super) fn cells(self, yaw: f64) -> impl Iterator<Item = Cell> {
        let view = View::new(yaw);
        let width = 2.34 / self.columns as f64;
        let height = 2.52 / self.rows as f64;
        (0..self.rows).flat_map(move |row| {
            let vertical = 1.17 - (row as f64 + 0.5) * height;
            (0..self.columns).map(move |column| {
                let horizontal = (column as f64 + 0.5) * width - 1.17;
                view.cell(horizontal, vertical, width, height)
            })
        })
    }
}

fn write_row(frame: &mut String, terminal: &Terminal, cells: impl Iterator<Item = Cell>) {
    let mut ink = Ink::Neutral;
    for cell in cells {
        if cell.ink != ink {
            frame.push_str(match cell.ink {
                Ink::Neutral => terminal.reset,
                Ink::Shadow => terminal.logo_shadow,
                Ink::Sector => terminal.logo_sector,
            });
            ink = cell.ink;
        }
        frame.push(char::from(cell.glyph));
    }
    if ink != Ink::Neutral {
        frame.push_str(terminal.reset);
    }
}

#[derive(Clone, Copy)]
struct View {
    pose: Pose,
    elevation: (f64, f64),
    direction: Vec3,
    light: Vec3,
}

impl View {
    fn new(yaw: f64) -> Self {
        let pose = Pose {
            yaw: yaw.sin_cos(),
            lean: LEAN.sin_cos(),
        };
        let (sin_elevation, cos_elevation) = 0.65_f64.sin_cos();
        let direction = pose.local(Vec3 {
            x: 0.0,
            y: cos_elevation,
            z: -sin_elevation,
        });
        let light = pose.local(Vec3 {
            x: -0.47,
            y: -0.72,
            z: 0.51,
        });
        Self {
            pose,
            elevation: (sin_elevation, cos_elevation),
            direction,
            light,
        }
    }

    fn cell(&self, horizontal: f64, vertical: f64, width: f64, height: f64) -> Cell {
        let mut intensity = 0.0;
        let mut tinted_samples = 0;
        let mut light_samples = 0;
        for x in [horizontal - width * 0.25, horizontal + width * 0.25] {
            for y in [vertical - height * 0.25, vertical + height * 0.25] {
                let (tone, sector) = self.sample(x, y);
                intensity += tone;
                tinted_samples += u8::from(sector);
                light_samples += u8::from(tone >= 0.77 && !sector);
            }
        }
        let sector = tinted_samples >= 2 && light_samples < 2;
        let coverage = intensity / 4.0;
        // Keep thin white details legible where they cover half a character cell.
        let tone = if light_samples >= 2 && !sector {
            coverage.max(0.75)
        } else {
            coverage
        };
        let lit_cell = (tone * (DENSITY.len() - 1) as f64).round() as usize;
        let glyph = if sector && lit_cell >= 6 {
            b'x'
        } else {
            DENSITY[lit_cell]
        };
        let ink = if glyph == b' ' || light_samples > 0 && !sector {
            Ink::Neutral
        } else if sector {
            Ink::Sector
        } else {
            Ink::Shadow
        };
        Cell { glyph, ink }
    }

    fn sample(&self, x: f64, y: f64) -> (f64, bool) {
        let (sin_elevation, cos_elevation) = self.elevation;
        let origin = self.pose.local(Vec3 {
            x,
            y: y * sin_elevation - 3.0 * cos_elevation,
            z: y * cos_elevation + 3.0 * sin_elevation,
        });
        let ray = Ray {
            origin,
            direction: self.direction,
        };
        ray.hit().map_or((0.0, false), |hit| shade(hit, self.light))
    }
}

#[derive(Clone, Copy)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Copy)]
struct Pose {
    yaw: (f64, f64),
    lean: (f64, f64),
}

impl Pose {
    fn local(&self, world: Vec3) -> Vec3 {
        // Invert the vertical-axis turn, then the tilt of the whole piece.
        let (sin_yaw, cos_yaw) = self.yaw;
        let (sin_lean, cos_lean) = self.lean;
        let y = world.y * cos_yaw - world.x * sin_yaw;
        Vec3 {
            x: world.x * cos_yaw + world.y * sin_yaw,
            y: y * cos_lean + world.z * sin_lean,
            z: world.z * cos_lean - y * sin_lean,
        }
    }
}

#[derive(Clone, Copy)]
struct Ray {
    origin: Vec3,
    direction: Vec3,
}

impl Ray {
    fn at(self, distance: f64) -> Vec3 {
        Vec3 {
            x: self.origin.x + distance * self.direction.x,
            y: self.origin.y + distance * self.direction.y,
            z: self.origin.z + distance * self.direction.z,
        }
    }

    fn hit(self) -> Option<Hit> {
        let a = self.direction.x * self.direction.x + self.direction.y * self.direction.y;
        let b = self.origin.x * self.direction.x + self.origin.y * self.direction.y;
        let radial = self.origin.x * self.origin.x + self.origin.y * self.origin.y;
        let side = |radius: f64| {
            let discriminant = b * b - a * (radial - radius * radius);
            (discriminant >= 0.0).then(|| {
                let distance = (-b - discriminant.sqrt()) / a;
                (distance, self.at(distance))
            })
        };
        let outer = side(1.0)?;
        let mut nearest = side(0.95)
            .filter(|(_, point)| (BASE..=TOP).contains(&point.z))
            .map(|(distance, point)| (distance, Hit::Groove(point)));
        for top in DISK_TOPS {
            let distance = (top - self.origin.z) / self.direction.z;
            let point = self.at(distance);
            let candidate = if point.x * point.x + point.y * point.y <= 1.0 {
                Some((distance, Hit::Face { point, top }))
            } else if (top - DISK_HEIGHT..=top).contains(&outer.1.z) {
                Some((
                    outer.0,
                    Hit::Wall {
                        point: outer.1,
                        top,
                    },
                ))
            } else {
                None
            };
            if let Some(candidate) = candidate
                && nearest.as_ref().is_none_or(|hit| candidate.0 <= hit.0)
            {
                nearest = Some(candidate);
            }
        }
        nearest.map(|(_, hit)| hit)
    }
}

enum Hit {
    Face { point: Vec3, top: f64 },
    Wall { point: Vec3, top: f64 },
    Groove(Vec3),
}

fn shade(hit: Hit, light: Vec3) -> (f64, bool) {
    let shine = |point: Vec3| (point.x * light.x + point.y * light.y).max(0.0);
    match hit {
        Hit::Face { point, top } => {
            let radius = point.x.hypot(point.y);
            if top != TOP || radius >= 0.95 {
                return (0.83 + 0.17 * shine(point), false);
            }
            let edge = point.y + point.x * 0.34;
            let hub = radius <= 0.23;
            let upright_line = point.y > 0.0 && point.x.abs() <= 0.038;
            let radial_line = point.x > 0.0 && edge.abs() <= 0.066;
            if hub || upright_line || radial_line {
                return (1.0, false);
            }
            if point.x > 0.0 && edge > 0.0 {
                return (0.79 + 0.10 * shine(point), true);
            }
            (0.16 + 0.09 * shine(point), false)
        }
        Hit::Wall { point, top } => {
            let shine = shine(point);
            let depth = top - point.z;
            if depth <= RIM_WIDTH {
                return (0.79 + 0.21 * shine, false);
            }
            (
                0.10 + 0.16 * shine + 0.09 * (1.0 - depth / DISK_HEIGHT),
                false,
            )
        }
        Hit::Groove(point) => (0.04 + 0.05 * shine(point), false),
    }
}
