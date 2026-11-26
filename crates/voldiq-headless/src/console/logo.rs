use super::terminal::Terminal;
use std::f64::consts::TAU;
use std::io::{self, Write};
use std::{
    thread,
    time::{Duration, Instant},
};

const WORDMARK: &str = "VOLDIQ  /  Disk space analyzer";
const DENSITY: &[u8] = b" .:-=+*#%@";
const REST_YAW: f64 = 0.64;
const LEAN: f64 = 0.19;
const LAYERS: [Cylinder; 4] = [
    Cylinder {
        radius: 0.76,
        bottom: -0.69,
        top: 0.43,
    },
    Cylinder {
        radius: 0.99,
        bottom: -0.7,
        top: -0.41,
    },
    Cylinder {
        radius: 1.0,
        bottom: -0.30,
        top: -0.01,
    },
    Cylinder {
        radius: 1.0,
        bottom: 0.09,
        top: 0.46,
    },
];

pub(super) fn write(out: &mut impl Write, terminal: &Terminal) -> io::Result<()> {
    let columns = terminal.width.saturating_sub(3).clamp(1, 42);
    let rows = (columns * 18).div_ceil(42);
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
    let (sin_elevation, cos_elevation) = 0.48_f64.sin_cos();
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
    let height = 2.48 / rows as f64;
    let mut frame = String::with_capacity((columns + 4) * rows);
    for row in 0..rows {
        frame.push_str("  ");
        let vertical = 1.17 - (row as f64 + 0.5) * height;
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
                    terminal.accent
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
    let mut sector = false;
    let intensity = if hit.face {
        let radius = hit.point.x.hypot(hit.point.y);
        let gloss =
            (1.0 - (hit.point.x * light.x + hit.point.y * light.y + 0.25).abs() * 1.5).max(0.0);
        if layer != LAYERS.len() - 1 {
            0.35 + gloss * 0.19
        } else if radius < 0.14 {
            0.95
        } else if radius < 0.22 {
            0.43
        } else if radius > 0.95 {
            0.78 + gloss * 0.2
        } else {
            sector = (hit.point.y.atan2(hit.point.x) + 0.85).abs() < 0.49;
            if sector {
                0.36 + gloss * 0.29
            } else {
                0.69 + gloss * 0.24
            }
        }
    } else {
        let shape = &LAYERS[layer];
        let diffuse = ((hit.point.x * light.x + hit.point.y * light.y) / shape.radius).max(0.0);
        if layer == 0 {
            0.12 + 0.12 * diffuse
        } else {
            let lip = (1.0 - (shape.top - hit.point.z) / 0.075).max(0.0);
            let shadow = (1.0 - (hit.point.z - shape.bottom) / 0.1).max(0.0);
            0.23 + layer as f64 * 0.033 + 0.43 * diffuse + 0.18 * lip - 0.13 * shadow
        }
    };
    (intensity, sector)
}
