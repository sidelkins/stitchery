//! Lettering: turns text in a system TrueType/OpenType font into stitches.
//!
//! Each glyph outline is flattened to polygons and filled with parallel rows
//! (nonzero winding rule, since fonts often overlap contours). Rows are chained into columns that overlap row to row, so
//! counters like the hole in "O" are sewn around rather than jumped across.

use crate::pattern::{Cmd, Pattern, Thread};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use ttf_parser::{Face, OutlineBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillStyle {
    /// One stitch across each row: smooth, for small or narrow lettering.
    Satin,
    /// Rows broken into staggered stitches: for large lettering.
    Tatami,
    /// Running stitch along the outline only.
    Outline,
}

#[derive(Clone, Debug)]
pub struct TextSpec {
    pub text: String,
    pub font: PathBuf,
    /// Capital letter height in mm.
    pub height_mm: f64,
    pub style: FillStyle,
    /// Fill row angle in degrees.
    pub angle: f64,
    /// Row spacing in mm.
    pub spacing_mm: f64,
    /// Extra letter spacing in mm.
    pub tracking_mm: f64,
    pub underlay: bool,
    pub outline: bool,
    pub rgb: [u8; 3],
}

impl Default for TextSpec {
    fn default() -> Self {
        TextSpec {
            text: "Hello".into(),
            font: PathBuf::new(),
            height_mm: 20.0,
            style: FillStyle::Satin,
            angle: 0.0,
            spacing_mm: 0.4,
            tracking_mm: 0.0,
            underlay: true,
            outline: false,
            rgb: [10, 85, 163],
        }
    }
}

#[derive(Clone, Debug)]
pub struct FontInfo {
    pub name: String,
    pub path: PathBuf,
}

/// Lists TrueType/OpenType fonts in the usual Linux font directories.
pub fn system_fonts() -> Vec<FontInfo> {
    let mut dirs = vec![PathBuf::from("/usr/share/fonts"), PathBuf::from("/usr/local/share/fonts")];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/share/fonts"));
        dirs.push(home.join(".fonts"));
    }
    let mut out = Vec::new();
    for d in dirs {
        walk(&d, &mut out, 0);
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

fn walk(dir: &Path, out: &mut Vec<FontInfo>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && depth < 6 {
            walk(&p, out, depth + 1);
            continue;
        }
        let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if ext != "ttf" && ext != "otf" {
            continue;
        }
        if let Some(name) = font_name(&p) {
            out.push(FontInfo { name, path: p });
        }
    }
}

fn font_name(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let face = Face::parse(&data, 0).ok()?;
    face.names()
        .into_iter()
        .filter(|n| n.name_id == ttf_parser::name_id::FULL_NAME && n.is_unicode())
        .find_map(|n| n.to_string())
}

type Poly = Vec<(f64, f64)>;

/// Longest satin stitch, in 0.1 mm.
const MAX_SATIN: f64 = 70.0;

/// Flattens glyph outlines into closed polygons.
struct Flatten {
    polys: Vec<Poly>,
    cur: Poly,
    tol: f64,
}

impl Flatten {
    fn last(&self) -> (f64, f64) {
        *self.cur.last().unwrap_or(&(0.0, 0.0))
    }
    fn steps(&self, len: f64) -> usize {
        ((len / self.tol).sqrt().ceil() as usize).clamp(1, 64)
    }
}

impl OutlineBuilder for Flatten {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.cur.push((x as f64, y as f64));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.cur.push((x as f64, y as f64));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x0, y0) = self.last();
        let (x1, y1, x, y) = (x1 as f64, y1 as f64, x as f64, y as f64);
        let n = self.steps((x1 - x0).hypot(y1 - y0) + (x - x1).hypot(y - y1));
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            self.cur.push((u * u * x0 + 2.0 * u * t * x1 + t * t * x, u * u * y0 + 2.0 * u * t * y1 + t * t * y));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x0, y0) = self.last();
        let (x1, y1, x2, y2, x, y) = (x1 as f64, y1 as f64, x2 as f64, y2 as f64, x as f64, y as f64);
        let n = self.steps((x1 - x0).hypot(y1 - y0) + (x2 - x1).hypot(y2 - y1) + (x - x2).hypot(y - y2));
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let px = u * u * u * x0 + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x;
            let py = u * u * u * y0 + 3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t * y;
            self.cur.push((px, py));
        }
    }
    fn close(&mut self) {
        if self.cur.len() >= 3 {
            self.polys.push(std::mem::take(&mut self.cur));
        }
        self.cur.clear();
    }
}

/// Glyph polygons laid out along a baseline, in 0.1 mm with +y down.
fn layout(spec: &TextSpec) -> Result<Vec<Vec<Poly>>> {
    let data = std::fs::read(&spec.font).with_context(|| format!("reading font {}", spec.font.display()))?;
    let face = Face::parse(&data, 0).context("unsupported font file")?;
    let cap = face.capital_height().filter(|&h| h > 0).map_or(face.units_per_em() as f64 * 0.7, |h| h as f64);
    let s = spec.height_mm * 10.0 / cap;
    let line_h = (face.ascender() - face.descender() + face.line_gap()) as f64 * s;
    let mut glyphs = Vec::new();
    let mut pen_y = 0.0;
    for line in spec.text.lines() {
        let mut pen_x = 0.0;
        let mut prev: Option<ttf_parser::GlyphId> = None;
        for ch in line.chars() {
            let Some(id) = face.glyph_index(ch) else { continue };
            if let (Some(p), Some(kern)) = (prev, face.tables().kern) {
                if let Some(k) = kern.subtables.into_iter().filter(|t| t.horizontal).find_map(|t| t.glyphs_kerning(p, id)) {
                    pen_x += k as f64 * s;
                }
            }
            let mut f = Flatten { polys: vec![], cur: vec![], tol: 0.15 / s };
            face.outline_glyph(id, &mut f);
            f.close();
            if !f.polys.is_empty() {
                let polys = f.polys.into_iter()
                    .map(|poly| poly.into_iter().map(|(x, y)| (pen_x + x * s, pen_y - y * s)).collect())
                    .collect();
                glyphs.push(polys);
            }
            pen_x += face.glyph_hor_advance(id).unwrap_or(0) as f64 * s + spec.tracking_mm * 10.0;
            prev = Some(id);
        }
        pen_y += line_h;
    }
    if glyphs.is_empty() {
        bail!("the font has no outlines for that text");
    }
    Ok(glyphs)
}

fn rotate(p: (f64, f64), deg: f64) -> (f64, f64) {
    let (sin, cos) = deg.to_radians().sin_cos();
    (p.0 * cos - p.1 * sin, p.0 * sin + p.1 * cos)
}

/// Fill segments per scan row, in rotated space: (y, [(x0, x1)]).
fn scan(polys: &[Poly], spacing: f64) -> Vec<(f64, Vec<(f64, f64)>)> {
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for p in polys {
        for q in p {
            lo = lo.min(q.1);
            hi = hi.max(q.1);
        }
    }
    let mut rows = Vec::new();
    let mut y = lo + spacing / 2.0;
    while y < hi {
        // Edge crossings with winding direction.
        let mut xs: Vec<(f64, i32)> = Vec::new();
        for p in polys {
            for i in 0..p.len() {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                if (a.1 <= y) != (b.1 <= y) {
                    let dir = if b.1 > a.1 { 1 } else { -1 };
                    xs.push((a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0), dir));
                }
            }
        }
        xs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut segs: Vec<(f64, f64)> = Vec::new();
        let (mut wind, mut start) = (0, 0.0);
        for (x, dir) in xs {
            if wind == 0 {
                start = x;
            }
            wind += dir;
            if wind == 0 && x - start > 0.5 {
                segs.push((start, x));
            }
        }
        rows.push((y, segs));
        y += spacing;
    }
    rows
}

/// Chains row segments into columns that sew without jumps. Each column is
/// a list of (row index, x0, x1).
fn columns(rows: &[(f64, Vec<(f64, f64)>)]) -> Vec<Vec<(usize, f64, f64)>> {
    let mut used: Vec<Vec<bool>> = rows.iter().map(|r| vec![false; r.1.len()]).collect();
    let mut cols = Vec::new();
    for r in 0..rows.len() {
        for s in 0..rows[r].1.len() {
            if used[r][s] {
                continue;
            }
            used[r][s] = true;
            let mut col = vec![(r, rows[r].1[s].0, rows[r].1[s].1)];
            let mut row = r + 1;
            while row < rows.len() {
                let (_, a, b) = *col.last().unwrap();
                let next = (0..rows[row].1.len())
                    .find(|&k| !used[row][k] && rows[row].1[k].0 < b && rows[row].1[k].1 > a);
                let Some(k) = next else { break };
                used[row][k] = true;
                col.push((row, rows[row].1[k].0, rows[row].1[k].1));
                row += 1;
            }
            cols.push(col);
        }
    }
    cols
}

/// Stitch points for a fill of `polys` at `angle` (degrees).
fn fill(polys: &[Poly], angle: f64, spacing: f64, stitch_len: Option<f64>) -> Vec<Vec<(f64, f64)>> {
    let rotated: Vec<Poly> = polys.iter().map(|p| p.iter().map(|&q| rotate(q, -angle)).collect()).collect();
    let rows = scan(&rotated, spacing);
    let mut out = Vec::new();
    // Travel longer than this inside a column ends the run (it becomes a jump).
    let max_travel = if stitch_len.is_some() { spacing * 2.0 + 20.0 } else { MAX_SATIN * 1.5 };
    for col in columns(&rows) {
        let mut pts: Vec<(f64, f64)> = Vec::new();
        for (i, &(r, a, b)) in col.iter().enumerate() {
            let y = rows[r].0;
            let (from, to) = match (stitch_len, pts.last()) {
                // Satin alternates edges every row.
                (None, _) => if i % 2 == 0 { (a, b) } else { (b, a) },
                // Fill starts each row at the end nearer the needle.
                (Some(_), Some(&(px, py))) if (b - px).hypot(y - py) < (a - px).hypot(y - py) => (b, a),
                _ => (a, b),
            };
            if let Some(&(px, py)) = pts.last() {
                // Travel is to the nearer end; the satin stitch across is not travel.
                if (a - px).hypot(y - py).min((b - px).hypot(y - py)) > max_travel {
                    out.push(std::mem::take(&mut pts));
                }
            }
            let Some(len) = stitch_len else {
                // Satin: one needle point per row; stitches longer than
                // MAX_SATIN are split so they don't snag.
                if let Some(&(px, py)) = pts.last() {
                    let n = ((from - px).hypot(y - py) / MAX_SATIN).ceil().max(1.0) as usize;
                    for k in 1..n {
                        let t = k as f64 / n as f64;
                        pts.push((px + (from - px) * t, py + (y - py) * t));
                    }
                }
                pts.push((from, y));
                continue;
            };
            pts.push((from, y));
            // Tatami: stagger needle points by a third of a stitch per row.
            let offset = (r % 3) as f64 * len / 3.0;
            let (lo, hi) = (a.min(b), a.max(b));
            let mut k = ((lo - offset) / len).floor() * len + offset;
            let mut inner = Vec::new();
            while k < hi {
                if k - lo > len * 0.3 && hi - k > len * 0.3 {
                    inner.push(k);
                }
                k += len;
            }
            if to < from {
                inner.reverse();
            }
            pts.extend(inner.into_iter().map(|x| (x, y)));
            pts.push((to, y));
        }
        out.push(pts);
    }
    out.retain(|r| r.len() > 1);
    out.into_iter().map(|r| r.into_iter().map(|q| rotate(q, angle)).collect()).collect()
}

/// Running stitch along each polygon.
fn outline(polys: &[Poly], len: f64) -> Vec<Vec<(f64, f64)>> {
    polys
        .iter()
        .map(|p| {
            let mut pts = vec![p[0]];
            let mut carry = 0.0;
            for i in 0..p.len() {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                let d = (b.0 - a.0).hypot(b.1 - a.1);
                let mut t = len - carry;
                while t < d {
                    pts.push((a.0 + (b.0 - a.0) * t / d, a.1 + (b.1 - a.1) * t / d));
                    t += len;
                }
                carry = (carry + d) % len;
            }
            pts.push(p[0]);
            pts
        })
        .collect()
}

/// Builds the lettering as a single-color design.
pub fn render(spec: &TextSpec) -> Result<Pattern> {
    let glyphs = layout(spec)?;
    let spacing = spec.spacing_mm.max(0.2) * 10.0;
    let mut p = Pattern::default();
    p.label = spec.text.lines().next().unwrap_or("").chars().take(16).collect();
    p.threads.push(Thread::new(spec.rgb, "Lettering"));
    let sew = |p: &mut Pattern, runs: Vec<Vec<(f64, f64)>>| {
        for run in runs {
            let Some(&(x, y)) = run.first() else { continue };
            let (px, py) = p.pos();
            if !p.stitches.is_empty() && (x - px).hypot(y - py) > 20.0 {
                p.trim();
            }
            if p.stitches.is_empty() || (x - px).hypot(y - py) > 0.0 {
                p.push(x, y, Cmd::Jump);
            }
            for &(x, y) in &run[1..] {
                p.push(x, y, Cmd::Stitch);
            }
        }
    };
    for g in &glyphs {
        if spec.style != FillStyle::Outline {
            if spec.underlay {
                // Sparse fill across the top-stitch direction holds the fabric.
                sew(&mut p, fill(g, spec.angle + 90.0, 20.0, Some(25.0)));
            }
            if spec.style == FillStyle::Tatami {
                sew(&mut p, fill(g, spec.angle, spacing, Some(40.0)));
            } else {
                // Zigzag rows: each edge is hit once per `spacing`.
                sew(&mut p, fill(g, spec.angle, spacing / 2.0, None));
            }
        }
        if spec.outline || spec.style == FillStyle::Outline {
            sew(&mut p, outline(g, 25.0));
        }
    }
    p.trim();
    p.cleanup();
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_fill_has_one_column_and_ring_has_more() {
        let sq = vec![vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]];
        assert_eq!(fill(&sq, 0.0, 4.0, None).len(), 1);
        let ring = vec![
            vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
            // Holes wind opposite to the outer contour, as in real fonts.
            vec![(30.0, 30.0), (30.0, 70.0), (70.0, 70.0), (70.0, 30.0)],
        ];
        let cols = fill(&ring, 0.0, 4.0, None);
        assert!(cols.len() >= 2 && cols.len() <= 4, "{}", cols.len());
    }

    #[test]
    fn renders_a_system_font() {
        let Some(font) = system_fonts().into_iter().next() else { return };
        let spec = TextSpec { text: "Ab".into(), font: font.path, ..Default::default() };
        let p = render(&spec).unwrap();
        let b = p.bounds();
        assert!(p.stitch_count() > 100);
        assert!(b.height() > 150.0 && b.height() < 300.0, "height {}", b.height());
        // Satin stitches must stay short enough not to snag.
        for w in p.stitches.windows(2) {
            if w[1].cmd == Cmd::Stitch {
                let d = (w[1].x - w[0].x).hypot(w[1].y - w[0].y);
                assert!(d <= MAX_SATIN + 1.0, "stitch of {d:.0} at {:?}", w[1]);
            }
        }
    }
}

