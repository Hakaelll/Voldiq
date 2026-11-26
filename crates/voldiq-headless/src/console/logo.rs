use super::terminal::Terminal;
use std::f64::consts::TAU;
use std::io::{self, Write};
use std::{
    thread,
    time::{Duration, Instant},
};

const WORDMARK: &str = "VOLDIQ  /  Disk space analyzer";
const DENSITY: &[u8] = b" .:-=+*#%@";
const REST_YAW: f64 = 0.13;
const LEAN: f64 = 0.14;
const LAYERS: [Cylinder; 4] = [
    Cylinder {
        radius: 0.78,
        bottom: -0.78,
        top: 0.43,
    },
    Cylinder {
        radius: 0.99,
        bottom: -0.85,
        top: -0.49,
    },
    Cylinder {
        radius: 1.0,
        bottom: -0.43,
        top: -0.05,
    },
    Cylinder {
        radius: 1.0,
        bottom: 0.02,
        top: 0.45,
    },
];

pub(super) fn write(out: &mut impl Write, terminal: &Terminal) -> io::Result<()> {
    let columns = terminal.width.saturating_sub(3).clamp(1, 48);
    let rows = (columns * 20).div_ceil(48);
    if terminal.motion && columns >= 26 && terminal.height > rows + 2 {
        const FRAMES: u32 = 30;
        const INTERVAL: Duration = Duration::from_millis(42);
        let start = Instant::now();
        for step in 0..FRAMES {
            let time = f64::from(step) / f64::from(FRAMES);
            // Ease the turn at both ends without changing the piece's lean.
            let rotation = time * time * (3.0 - 2.0 * time);
            draw(
                out,
                terminal,
                columns,
                rows,
                REST_YAW + TAU * (rotation - 1.0),
                false,
            )?;
            out.flush()?;
            thread::sleep(
                (start + INTERVAL * (step + 1)).saturating_duration_since(Instant::now()),
            );
            write!(out, "\x1b[{rows}A\r")?;
        }
    }
    draw(out, terminal, columns, rows, REST_YAW, true)?;
    writeln!(out)?;
    out.flush()
}

fn draw(
    out: &mut impl Write,
    terminal: &Terminal,
    columns: usize,
    rows: usize,
    yaw: f64,
    settled: bool,
) -> io::Result<()> {
    let pose = Pose {
        yaw: yaw.sin_cos(),
        lean: LEAN.sin_cos(),
    };
    let (sin_elevation, cos_elevation) = 0.43_f64.sin_cos();
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
    let view = View {
        pose,
        elevation: (sin_elevation, cos_elevation),
        direction,
        light,
    };
    let beside_mark = columns + WORDMARK.len() + 7 <= terminal.width;
    let width = 2.64 / columns as f64;
    let height = 2.62 / rows as f64;
    let mut frame = String::with_capacity((columns + 4) * rows);
    for row in 0..rows {
        frame.push_str("  ");
        let vertical = 1.18 - (row as f64 + 0.5) * height;
        let mut tinted = false;
        for column in 0..columns {
            let horizontal = (column as f64 + 0.5) * width - 1.32;
            let mut intensity = 0.0;
            let mut tinted_samples = 0;
            for x in [horizontal - width * 0.25, horizontal + width * 0.25] {
                for y in [vertical - height * 0.25, vertical + height * 0.25] {
                    let (tone, sector) = view.sample(x, y);
                    intensity += tone;
                    tinted_samples += u8::from(sector);
                }
            }
            let sector = tinted_samples >= 2;
            let lit_cell =
                (intensity.clamp(0.0, 4.0) * (DENSITY.len() - 1) as f64 / 4.0).round() as usize;
            if sector != tinted {
                frame.push_str(if sector {
                    terminal.logo_sector
                } else {
                    terminal.reset
                });
                tinted = sector;
            }
            frame.push(char::from(DENSITY[lit_cell]));
        }
        if tinted {
            frame.push_str(terminal.reset);
        }
        if settled && beside_mark && row == rows / 2 {
            frame.push_str("    ");
            frame.push_str(terminal.strong);
            frame.push_str(WORDMARK);
            frame.push_str(terminal.reset);
        }
        frame.push('\n');
    }
    out.write_all(frame.as_bytes())?;
    if settled && !beside_mark {
        writeln!(out, "  {}{WORDMARK}{}", terminal.strong, terminal.reset)?;
    }
    Ok(())
}

struct View {
    pose: Pose,
    elevation: (f64, f64),
    direction: Vec3,
    light: Vec3,
}

impl View {
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
        LAYERS
            .iter()
            .enumerate()
            .filter_map(|(layer, cylinder)| cylinder.hit(ray).map(|hit| (layer, hit)))
            .min_by(|(_, first), (_, second)| first.distance.total_cmp(&second.distance))
            .map_or((0.0, false), |(layer, hit)| shade(layer, hit, self.light))
    }
}

#[derive(Clone, Copy)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

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
}

struct Cylinder {
    radius: f64,
    bottom: f64,
    top: f64,
}

struct Hit {
    distance: f64,
    point: Vec3,
    face: bool,
}

impl Cylinder {
    fn hit(&self, ray: Ray) -> Option<Hit> {
        let distance = (self.top - ray.origin.z) / ray.direction.z;
        let top = ray.at(distance);
        let radius_squared = self.radius * self.radius;
        let face = (top.x * top.x + top.y * top.y <= radius_squared).then_some(Hit {
            distance,
            point: top,
            face: true,
        });

        let a = ray.direction.x * ray.direction.x + ray.direction.y * ray.direction.y;
        let b = ray.origin.x * ray.direction.x + ray.origin.y * ray.direction.y;
        let c = ray.origin.x * ray.origin.x + ray.origin.y * ray.origin.y - radius_squared;
        let discriminant = b * b - a * c;
        if discriminant < 0.0 {
            return face;
        }
        let distance = (-b - discriminant.sqrt()) / a;
        let side = ray.at(distance);
        if (self.bottom..=self.top).contains(&side.z)
            && face.as_ref().is_none_or(|hit| distance < hit.distance)
        {
            return Some(Hit {
                distance,
                point: side,
                face: false,
            });
        }
        face
    }
}

fn shade(layer: usize, hit: Hit, light: Vec3) -> (f64, bool) {
    let point = hit.point;
    let shine = (point.x * light.x + point.y * light.y).max(0.0);
    if layer == 0 {
        return (0.08 + shine * 0.10, false);
    }
    let shape = &LAYERS[layer];
    let top_disc = layer == LAYERS.len() - 1;
    if hit.face && top_disc {
        let radius = point.x.hypot(point.y);
        let separator = point.y + point.x * 0.22;
        if radius < 0.26
            || (point.y > 0.0 && point.x.abs() < 0.043)
            || (point.x > 0.0 && separator.abs() < 0.048)
        {
            return (0.96, false);
        }
        if point.x > 0.0 && separator > 0.0 {
            return (0.68 + shine * 0.26, true);
        }
        if radius > 0.94 {
            return (0.79 + shine * 0.17, false);
        }
        return (0.22 + shine * 0.20, false);
    }
    if hit.face {
        return (0.66 + shine * 0.16, false);
    }
    if point.z > shape.top - 0.081 {
        return (0.87 + shine * 0.12, false);
    }
    if layer == 1 && point.z < shape.bottom + 0.052 {
        return (0.49 + shine * 0.16, false);
    }
    (0.21 + shine * 0.22, false)
}
