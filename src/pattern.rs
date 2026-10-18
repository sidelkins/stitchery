//! In-memory embroidery design model.
//!
//! Coordinates are absolute, in units of 0.1 mm, with +y pointing down
//! (screen orientation). File formats with +y up flip on read/write.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// Needle penetration at (x, y); thread is sewn from the previous position.
    Stitch,
    /// Frame move to (x, y) without sewing.
    Jump,
    /// Cut the thread at the current position.
    Trim,
    /// Switch to the next thread at the current position.
    ColorChange,
}

#[derive(Clone, Copy, Debug)]
pub struct Stitch {
    pub x: f64,
    pub y: f64,
    pub cmd: Cmd,
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub rgb: [u8; 3],
    pub name: String,
    pub code: String,
}

impl Thread {
    pub fn new(rgb: [u8; 3], name: impl Into<String>) -> Self {
        Thread { rgb, name: name.into(), code: String::new() }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Pattern {
    pub stitches: Vec<Stitch>,
    pub threads: Vec<Thread>,
    pub label: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Bounds {
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }
}

impl Pattern {
    pub fn push(&mut self, x: f64, y: f64, cmd: Cmd) {
        self.stitches.push(Stitch { x, y, cmd });
    }

    /// Position of the last record, or the origin for an empty design.
    pub fn pos(&self) -> (f64, f64) {
        self.stitches.last().map_or((0.0, 0.0), |s| (s.x, s.y))
    }

    pub fn color_change(&mut self) {
        let (x, y) = self.pos();
        self.push(x, y, Cmd::ColorChange);
    }

    pub fn trim(&mut self) {
        let (x, y) = self.pos();
        self.push(x, y, Cmd::Trim);
    }

    /// Number of color blocks the stitch stream uses.
    pub fn color_count(&self) -> usize {
        1 + self.stitches.iter().filter(|s| s.cmd == Cmd::ColorChange).count()
    }

    /// Makes sure there is one thread per color block, inventing colors as needed.
    pub fn ensure_threads(&mut self) {
        let n = self.color_count();
        while self.threads.len() < n {
            let i = self.threads.len();
            let rgb = crate::threads::DEFAULT_CYCLE[i % crate::threads::DEFAULT_CYCLE.len()];
            self.threads.push(Thread::new(rgb, format!("Color {}", i + 1)));
        }
    }

    pub fn stitch_count(&self) -> usize {
        self.stitches.iter().filter(|s| s.cmd == Cmd::Stitch).count()
    }

    pub fn jump_count(&self) -> usize {
        self.stitches.iter().filter(|s| s.cmd == Cmd::Jump).count()
    }

    pub fn trim_count(&self) -> usize {
        self.stitches.iter().filter(|s| s.cmd == Cmd::Trim).count()
    }

    /// Bounds of sewn stitches only (jumps excluded), matching what the machine stitches.
    pub fn bounds(&self) -> Bounds {
        let mut it = self.stitches.iter().filter(|s| s.cmd == Cmd::Stitch);
        let Some(first) = it.next() else { return Bounds::default() };
        let mut b = Bounds { min_x: first.x, min_y: first.y, max_x: first.x, max_y: first.y };
        for s in it {
            b.min_x = b.min_x.min(s.x);
            b.min_y = b.min_y.min(s.y);
            b.max_x = b.max_x.max(s.x);
            b.max_y = b.max_y.max(s.y);
        }
        b
    }

    pub fn translate(&mut self, dx: f64, dy: f64) {
        for s in &mut self.stitches {
            s.x += dx;
            s.y += dy;
        }
    }

    /// Moves the design so its bounding box is centered on the origin.
    pub fn center(&mut self) {
        let b = self.bounds();
        self.translate(-(b.min_x + b.max_x) / 2.0, -(b.min_y + b.max_y) / 2.0);
    }

    /// Scales stitch positions about the design center. This is a plain
    /// geometric resize: stitch count is unchanged, so density changes.
    pub fn scale(&mut self, sx: f64, sy: f64) {
        let b = self.bounds();
        let (cx, cy) = ((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0);
        for s in &mut self.stitches {
            s.x = cx + (s.x - cx) * sx;
            s.y = cy + (s.y - cy) * sy;
        }
    }

    /// Rotates about the design center by `deg` degrees clockwise (on screen).
    pub fn rotate(&mut self, deg: f64) {
        let b = self.bounds();
        let (cx, cy) = ((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0);
        let (sin, cos) = deg.to_radians().sin_cos();
        for s in &mut self.stitches {
            let (x, y) = (s.x - cx, s.y - cy);
            s.x = cx + x * cos - y * sin;
            s.y = cy + x * sin + y * cos;
        }
    }

    pub fn mirror_horizontal(&mut self) {
        let b = self.bounds();
        let c = b.min_x + b.max_x;
        for s in &mut self.stitches {
            s.x = c - s.x;
        }
    }

    pub fn mirror_vertical(&mut self) {
        let b = self.bounds();
        let c = b.min_y + b.max_y;
        for s in &mut self.stitches {
            s.y = c - s.y;
        }
    }

    /// Appends `other` after this design: trim, jump to its start, change color.
    pub fn merge(&mut self, other: &Pattern) {
        let mut other = other.clone();
        other.ensure_threads();
        if self.stitches.is_empty() {
            *self = other;
            return;
        }
        self.ensure_threads();
        self.trim();
        self.color_change();
        for s in &other.stitches {
            self.stitches.push(*s);
        }
        self.threads.extend(other.threads);
    }

    /// Index of the color block each record belongs to.
    pub fn color_indices(&self) -> Vec<usize> {
        let mut c = 0;
        self.stitches
            .iter()
            .map(|s| {
                if s.cmd == Cmd::ColorChange {
                    c += 1;
                }
                c
            })
            .collect()
    }

    /// Removes zero-length stitches and redundant consecutive color changes.
    pub fn cleanup(&mut self) {
        let mut out: Vec<Stitch> = Vec::with_capacity(self.stitches.len());
        let mut pending_cc = false;
        let mut seen_stitch = false;
        for s in &self.stitches {
            match s.cmd {
                Cmd::ColorChange => {
                    // A color change before any stitch, or doubled, stitches nothing.
                    if seen_stitch {
                        pending_cc = true;
                    }
                }
                Cmd::Stitch => {
                    if pending_cc {
                        let (x, y) = out.last().map_or((0.0, 0.0), |p| (p.x, p.y));
                        out.push(Stitch { x, y, cmd: Cmd::ColorChange });
                        pending_cc = false;
                    }
                    if let Some(p) = out.last() {
                        if p.cmd == Cmd::Stitch && p.x == s.x && p.y == s.y {
                            continue;
                        }
                    }
                    seen_stitch = true;
                    out.push(*s);
                }
                _ => out.push(*s),
            }
        }
        self.stitches = out;
    }

    /// Relative integer moves for writers, splitting moves longer than `max`
    /// (in 0.1 mm) into several records. Positions are rounded absolutely so
    /// rounding error never accumulates.
    pub fn relative(&self, max: i32) -> Vec<(i32, i32, Cmd)> {
        let mut out = Vec::new();
        let (mut px, mut py) = (0i32, 0i32);
        for s in &self.stitches {
            let (tx, ty) = (s.x.round() as i32, s.y.round() as i32);
            match s.cmd {
                Cmd::Trim | Cmd::ColorChange => out.push((0, 0, s.cmd)),
                Cmd::Stitch | Cmd::Jump => {
                    let (dx, dy) = (tx - px, ty - py);
                    let steps = ((dx.abs().max(dy.abs()) + max - 1) / max).max(1);
                    let (mut lx, mut ly) = (0, 0);
                    for i in 1..=steps {
                        let nx = dx * i / steps;
                        let ny = dy * i / steps;
                        out.push((nx - lx, ny - ly, s.cmd));
                        lx = nx;
                        ly = ny;
                    }
                    px = tx;
                    py = ty;
                }
            }
        }
        out
    }
}
