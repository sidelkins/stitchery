//! Machine embroidery file formats, implemented from public format notes.
//!
//! | Format | Read | Write |
//! |--------|------|-------|
//! | DST (Tajima)       | yes | yes |
//! | EXP (Melco/Bernina)| yes | yes |
//! | PEC (Brother)      | yes | yes |
//! | PES (Brother)      | yes (PEC section) | yes (v1 header + PEC) |
//! | JEF (Janome)       | yes | yes |
//! | SVG                | no  | yes |

use crate::pattern::{Cmd, Pattern, Thread};
use crate::threads;
use anyhow::{bail, Context, Result};
use std::path::Path;

pub const READ_EXTS: &[&str] = &["dst", "exp", "pec", "pes", "jef"];
pub const WRITE_EXTS: &[&str] = &["dst", "exp", "pec", "pes", "jef", "svg"];

fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

pub fn read(path: &Path) -> Result<Pattern> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut p = match ext(path).as_str() {
        "dst" => read_dst(&data)?,
        "exp" => read_exp(&data)?,
        "pec" => read_pec_file(&data)?,
        "pes" => read_pes(&data)?,
        "jef" => read_jef(&data)?,
        e => bail!("unsupported input format '.{e}'"),
    };
    if p.label.trim().is_empty() {
        p.label = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    }
    p.cleanup();
    p.ensure_threads();
    Ok(p)
}

pub fn write(path: &Path, p: &Pattern) -> Result<()> {
    let mut p = p.clone();
    p.ensure_threads();
    let data = match ext(path).as_str() {
        "dst" => write_dst(&p),
        "exp" => write_exp(&p),
        "pec" => write_pec_file(&p),
        "pes" => write_pes(&p),
        "jef" => write_jef(&p),
        "svg" => write_svg(&p).into_bytes(),
        e => bail!("unsupported output format '.{e}'"),
    };
    std::fs::write(path, data).with_context(|| format!("writing {}", path.display()))
}

struct Cursor<'a> {
    d: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn new(d: &'a [u8], i: usize) -> Self {
        Cursor { d, i }
    }
    fn u8(&mut self) -> Option<u8> {
        let v = *self.d.get(self.i)?;
        self.i += 1;
        Some(v)
    }
}

fn le32(d: &[u8], at: usize) -> Result<u32> {
    let b = d.get(at..at + 4).context("file truncated")?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}

fn label_bytes(label: &str, n: usize) -> Vec<u8> {
    let mut v: Vec<u8> = label.bytes().filter(|b| b.is_ascii_graphic() || *b == b' ').take(n).collect();
    v.resize(n, b' ');
    v
}

// ---------------------------------------------------------------- DST

fn read_dst(d: &[u8]) -> Result<Pattern> {
    if d.len() < 512 {
        bail!("DST file too short");
    }
    let mut p = Pattern::default();
    if d.starts_with(b"LA:") {
        p.label = String::from_utf8_lossy(&d[3..19]).trim().to_string();
    }
    let (mut x, mut y) = (0.0, 0.0);
    for r in d[512..].chunks_exact(3) {
        let (b0, b1, b2) = (r[0], r[1], r[2]);
        if b2 & 0xF3 == 0xF3 {
            break;
        }
        let bit = |b: u8, m: u8, v: i32| if b & m != 0 { v } else { 0 };
        let dx = bit(b0, 0x01, 1) - bit(b0, 0x02, 1) + bit(b0, 0x04, 9) - bit(b0, 0x08, 9)
            + bit(b1, 0x01, 3) - bit(b1, 0x02, 3) + bit(b1, 0x04, 27) - bit(b1, 0x08, 27)
            + bit(b2, 0x04, 81) - bit(b2, 0x08, 81);
        let dy = bit(b0, 0x80, 1) - bit(b0, 0x40, 1) + bit(b0, 0x20, 9) - bit(b0, 0x10, 9)
            + bit(b1, 0x80, 3) - bit(b1, 0x40, 3) + bit(b1, 0x20, 27) - bit(b1, 0x10, 27)
            + bit(b2, 0x20, 81) - bit(b2, 0x10, 81);
        x += dx as f64;
        y -= dy as f64;
        if b2 & 0xC3 == 0xC3 {
            p.color_change();
            if dx != 0 || dy != 0 {
                p.push(x, y, Cmd::Jump);
            }
        } else if b2 & 0x80 != 0 {
            p.push(x, y, Cmd::Jump);
        } else {
            p.push(x, y, Cmd::Stitch);
        }
    }
    Ok(p)
}

/// Balanced-ternary digits for weights 81, 27, 9, 3, 1 (|v| <= 121).
fn ternary(mut v: i32) -> [i32; 5] {
    let mut out = [0; 5];
    for (k, w) in [81, 27, 9, 3, 1].into_iter().enumerate() {
        out[k] = if v > w / 2 { 1 } else if v < -(w / 2) { -1 } else { 0 };
        v -= out[k] * w;
    }
    out
}

fn dst_record(dx: i32, dy: i32, flags: u8) -> [u8; 3] {
    let tx = ternary(dx);
    let ty = ternary(dy);
    let mut b = [0u8, 0u8, flags | 0x03];
    // (byte, positive mask, negative mask) for weights 81, 27, 9, 3, 1.
    let xm = [(2, 0x04, 0x08), (1, 0x04, 0x08), (0, 0x04, 0x08), (1, 0x01, 0x02), (0, 0x01, 0x02)];
    let ym = [(2, 0x20, 0x10), (1, 0x20, 0x10), (0, 0x20, 0x10), (1, 0x80, 0x40), (0, 0x80, 0x40)];
    for k in 0..5 {
        let (i, pm, nm) = xm[k];
        if tx[k] > 0 { b[i] |= pm } else if tx[k] < 0 { b[i] |= nm }
        let (i, pm, nm) = ym[k];
        if ty[k] > 0 { b[i] |= pm } else if ty[k] < 0 { b[i] |= nm }
    }
    b
}

fn write_dst(p: &Pattern) -> Vec<u8> {
    let mut body = Vec::new();
    let mut count = 0;
    let mut pending_jump = false;
    for (dx, dy, cmd) in p.relative(121) {
        match cmd {
            Cmd::Stitch => {
                body.extend(dst_record(dx, -dy, 0));
                pending_jump = false;
            }
            Cmd::Jump => {
                body.extend(dst_record(dx, -dy, 0x80));
                pending_jump = true;
            }
            Cmd::ColorChange => body.extend(dst_record(0, 0, 0xC0)),
            // DST has no trim command; machines trim on a run of jumps.
            Cmd::Trim => {
                if !pending_jump {
                    body.extend(dst_record(2, 2, 0x80));
                    body.extend(dst_record(-4, -4, 0x80));
                    body.extend(dst_record(2, 2, 0x80));
                }
            }
        }
        count += 1;
    }
    body.extend([0, 0, 0xF3]);
    let b = p.bounds();
    let mut h = Vec::new();
    h.extend(b"LA:");
    h.extend(label_bytes(&p.label, 16));
    h.push(b'\r');
    h.extend(format!("ST:{:7}\r", count).bytes());
    h.extend(format!("CO:{:3}\r", p.color_count() - 1).bytes());
    h.extend(format!("+X:{:5}\r", b.max_x.max(0.0).round() as i32).bytes());
    h.extend(format!("-X:{:5}\r", (-b.min_x).max(0.0).round() as i32).bytes());
    // DST's +Y is up; our y is down.
    h.extend(format!("+Y:{:5}\r", (-b.min_y).max(0.0).round() as i32).bytes());
    h.extend(format!("-Y:{:5}\r", b.max_y.max(0.0).round() as i32).bytes());
    let (ex, ey) = p.stitches.last().map_or((0, 0), |s| (s.x.round() as i32, -s.y.round() as i32));
    h.extend(format!("AX:{}{:5}\r", if ex < 0 { '-' } else { '+' }, ex.abs()).bytes());
    h.extend(format!("AY:{}{:5}\r", if ey < 0 { '-' } else { '+' }, ey.abs()).bytes());
    h.extend(b"MX:+    0\rMY:+    0\rPD:******\r");
    h.push(0x1A);
    h.resize(512, b' ');
    h.extend(body);
    h
}

// ---------------------------------------------------------------- EXP

fn read_exp(d: &[u8]) -> Result<Pattern> {
    let mut p = Pattern::default();
    let (mut x, mut y) = (0.0, 0.0);
    let mut c = Cursor::new(d, 0);
    let sb = |b: u8| b as i8 as f64;
    while let (Some(a), Some(b)) = (c.u8(), c.u8()) {
        if a != 0x80 {
            x += sb(a);
            y -= sb(b);
            p.push(x, y, Cmd::Stitch);
            continue;
        }
        let (Some(dx), Some(dy)) = (c.u8(), c.u8()) else { break };
        match b {
            0x01 => {
                p.color_change();
                if dx != 0 || dy != 0 {
                    x += sb(dx);
                    y -= sb(dy);
                    p.push(x, y, Cmd::Jump);
                }
            }
            0x02 | 0x04 => {
                x += sb(dx);
                y -= sb(dy);
                p.push(x, y, Cmd::Jump);
            }
            0x80 => p.trim(),
            _ => {}
        }
    }
    Ok(p)
}

fn write_exp(p: &Pattern) -> Vec<u8> {
    let mut o = Vec::new();
    for (dx, dy, cmd) in p.relative(127) {
        let (bx, by) = (dx as i8 as u8, (-dy) as i8 as u8);
        match cmd {
            Cmd::Stitch => o.extend([bx, by]),
            Cmd::Jump => o.extend([0x80, 0x04, bx, by]),
            Cmd::ColorChange => o.extend([0x80, 0x01, 0, 0]),
            Cmd::Trim => o.extend([0x80, 0x80, 0x07, 0x00]),
        }
    }
    o
}

// ---------------------------------------------------------------- PEC

fn read_pec_file(d: &[u8]) -> Result<Pattern> {
    if !d.starts_with(b"#PEC0001") {
        bail!("not a PEC file");
    }
    read_pec(d, 8)
}

/// Parses a PEC block starting at `start` (also used inside PES).
fn read_pec(d: &[u8], start: usize) -> Result<Pattern> {
    let mut p = Pattern::default();
    let label = d.get(start + 3..start + 19).context("PEC truncated")?;
    p.label = String::from_utf8_lossy(label).trim().to_string();
    let ncolors = *d.get(start + 48).context("PEC truncated")? as usize + 1;
    let colors = d.get(start + 49..start + 49 + ncolors).context("PEC truncated")?;
    p.threads = colors.iter().map(|&i| threads::pec_thread(i)).collect();

    let mut c = Cursor::new(d, start + 0x210);
    let (mut x, mut y) = (0.0, 0.0);
    // Returns (delta, jump, trim).
    fn coord(c: &mut Cursor, first: u8) -> Option<(f64, bool, bool)> {
        if first & 0x80 != 0 {
            let lo = c.u8()?;
            let mut v = (((first & 0x0F) as i32) << 8) | lo as i32;
            if v & 0x800 != 0 {
                v -= 0x1000;
            }
            Some((v as f64, first & 0x10 != 0, first & 0x20 != 0))
        } else {
            let v = if first > 63 { first as i32 - 128 } else { first as i32 };
            Some((v as f64, false, false))
        }
    }
    while let Some(b0) = c.u8() {
        if b0 == 0xFF {
            break;
        }
        if b0 == 0xFE {
            if c.u8() == Some(0xB0) {
                c.u8();
                p.color_change();
                continue;
            }
            break;
        }
        let Some((dx, jx, tx)) = coord(&mut c, b0) else { break };
        let Some(b1) = c.u8() else { break };
        let Some((dy, jy, ty)) = coord(&mut c, b1) else { break };
        x += dx;
        y += dy;
        if tx || ty {
            p.trim();
        }
        p.push(x, y, if jx || jy || tx || ty { Cmd::Jump } else { Cmd::Stitch });
    }
    Ok(p)
}

fn pec_coord(o: &mut Vec<u8>, v: i32, long: bool, flag: u16) {
    if !long && (-64..=63).contains(&v) {
        o.push((v & 0x7F) as u8);
    } else {
        let w = 0x8000 | flag | (v as u16 & 0x0FFF);
        o.extend(w.to_be_bytes());
    }
}

/// 48x38 1-bit thumbnail, LSB-first, 6 bytes per row.
fn pec_thumbnail(p: &Pattern, block: Option<usize>) -> [u8; 228] {
    let mut g = [0u8; 228];
    let mut set = |x: i32, y: i32| {
        if (0..48).contains(&x) && (0..38).contains(&y) {
            g[(y * 6 + x / 8) as usize] |= 1 << (x % 8);
        }
    };
    for x in 4..44 {
        set(x, 1);
        set(x, 36);
    }
    for y in 2..36 {
        set(3, y);
        set(44, y);
    }
    let b = p.bounds();
    let s = (38.0 / b.width().max(1.0)).min(31.0 / b.height().max(1.0));
    let ci = p.color_indices();
    for (i, st) in p.stitches.iter().enumerate() {
        if st.cmd == Cmd::Stitch && block.is_none_or(|bk| ci[i] == bk) {
            let x = 5.0 + (st.x - b.min_x) * s + (38.0 - b.width() * s) / 2.0;
            let y = 3.0 + (st.y - b.min_y) * s + (31.0 - b.height() * s) / 2.0;
            set(x as i32, y as i32);
        }
    }
    g
}

fn pec_block(p: &Pattern) -> Vec<u8> {
    let colors: Vec<u8> = p.threads.iter().take(p.color_count()).map(|t| {
        // Keep an exact palette index when the thread came from one.
        match t.code.parse::<u8>() {
            Ok(i) if (1..65).contains(&i) && threads::PEC_PALETTE[i as usize].0 == t.rgb => i,
            _ => threads::nearest_pec(t.rgb),
        }
    }).collect();
    let mut o = Vec::new();
    o.extend(b"LA:");
    o.extend(label_bytes(&p.label, 16));
    o.push(b'\r');
    o.extend([b' '; 12]);
    o.extend([0xFF, 0x00, 0x06, 0x26]);
    o.extend([b' '; 12]);
    o.push((colors.len() - 1) as u8);
    o.extend(&colors);
    o.resize(0x200, b' ');
    o.extend([0, 0]);
    let len_at = o.len();
    o.extend([0, 0, 0, 0x31, 0xFF, 0xF0]);
    let b = p.bounds();
    o.extend((b.width().round() as u16).to_le_bytes());
    o.extend((b.height().round() as u16).to_le_bytes());
    o.extend(0x1E0u16.to_le_bytes());
    o.extend(0x1B0u16.to_le_bytes());

    // PEC coordinates are relative to the design's top-left corner.
    let mut q = p.clone();
    q.translate(-b.min_x, -b.min_y);
    let mut toggle = 2u8;
    let mut trim_next = false;
    for (dx, dy, cmd) in q.relative(2047) {
        match cmd {
            Cmd::Stitch => {
                pec_coord(&mut o, dx, false, 0);
                pec_coord(&mut o, dy, false, 0);
            }
            Cmd::Jump => {
                let flag = if trim_next { 0x2000 } else { 0x1000 };
                pec_coord(&mut o, dx, true, flag);
                pec_coord(&mut o, dy, true, flag);
                trim_next = false;
            }
            Cmd::Trim => trim_next = true,
            Cmd::ColorChange => {
                o.extend([0xFE, 0xB0, toggle]);
                toggle = if toggle == 2 { 1 } else { 2 };
            }
        }
    }
    o.extend([0xFF, 0x00]);
    let graphics = o.len() - 0x200;
    o[len_at..len_at + 3].copy_from_slice(&(graphics as u32).to_le_bytes()[..3]);
    o.extend(pec_thumbnail(p, None));
    for i in 0..colors.len() {
        o.extend(pec_thumbnail(p, Some(i)));
    }
    o
}

fn write_pec_file(p: &Pattern) -> Vec<u8> {
    let mut o = b"#PEC0001".to_vec();
    o.extend(pec_block(p));
    o
}

// ---------------------------------------------------------------- PES

fn read_pes(d: &[u8]) -> Result<Pattern> {
    if !d.starts_with(b"#PES") {
        bail!("not a PES file");
    }
    let pec = le32(d, 8)? as usize;
    read_pec(d, pec)
}

/// Writes a minimal version 1 PES: header, empty PES section, then PEC.
/// Brother/Baby Lock machines stitch from the PEC section.
fn write_pes(p: &Pattern) -> Vec<u8> {
    let mut o = b"#PES0001".to_vec();
    o.extend(0x16u32.to_le_bytes());
    o.extend([0u8; 10]);
    o.extend(pec_block(p));
    o
}

// ---------------------------------------------------------------- JEF

fn read_jef(d: &[u8]) -> Result<Pattern> {
    let start = le32(d, 0)? as usize;
    let ncolors = le32(d, 24)? as usize;
    let mut p = Pattern::default();
    for i in 0..ncolors.min(256) {
        let idx = le32(d, 116 + 4 * i).unwrap_or(0) as usize;
        let rgb = threads::DEFAULT_CYCLE[idx % threads::DEFAULT_CYCLE.len()];
        p.threads.push(Thread { rgb, name: format!("Janome #{idx}"), code: idx.to_string() });
    }
    let mut c = Cursor::new(d, start);
    let (mut x, mut y) = (0.0, 0.0);
    let sb = |b: u8| b as i8 as f64;
    while let (Some(a), Some(b)) = (c.u8(), c.u8()) {
        if a != 0x80 {
            x += sb(a);
            y -= sb(b);
            p.push(x, y, Cmd::Stitch);
            continue;
        }
        if b == 0x10 {
            break;
        }
        let (Some(dx), Some(dy)) = (c.u8(), c.u8()) else { break };
        if b == 0x01 {
            p.color_change();
        }
        if b == 0x02 || dx != 0 || dy != 0 {
            x += sb(dx);
            y -= sb(dy);
            p.push(x, y, Cmd::Jump);
        }
    }
    Ok(p)
}

fn write_jef(p: &Pattern) -> Vec<u8> {
    let mut body = Vec::new();
    for (dx, dy, cmd) in p.relative(127) {
        let (bx, by) = (dx as i8 as u8, (-dy) as i8 as u8);
        match cmd {
            Cmd::Stitch => body.extend([bx, by]),
            Cmd::Jump => body.extend([0x80, 0x02, bx, by]),
            Cmd::ColorChange => body.extend([0x80, 0x01, 0, 0]),
            // Janome machines trim on a jump; mark one explicitly.
            Cmd::Trim => body.extend([0x80, 0x02, 0, 0]),
        }
    }
    body.extend([0x80, 0x10]);

    let n = p.color_count();
    let b = p.bounds();
    let (hw, hh) = ((b.width() / 2.0).ceil() as i32, (b.height() / 2.0).ceil() as i32);
    // (code, half width, half height) in 0.1 mm, smallest first.
    let hoops = [(2, 250, 250), (1, 550, 550), (0, 630, 550), (3, 700, 1000), (4, 1150, 1000)];
    let hoop = hoops.iter().find(|h| hw <= h.1 && hh <= h.2).map_or(4, |h| h.0);
    let mut o = Vec::new();
    let offset = 116 + 8 * n as u32;
    o.extend(offset.to_le_bytes());
    o.extend(0x14u32.to_le_bytes());
    o.extend(b"20260101000000");
    o.extend([0, 0]);
    o.extend((n as u32).to_le_bytes());
    o.extend(((body.len() / 2) as u32).to_le_bytes());
    o.extend((hoop as u32).to_le_bytes());
    for v in [hw, hh, hw, hh] {
        o.extend(v.to_le_bytes());
    }
    // Clearance to the edges of hoops 110x110, 50x50, 140x200, 126x110.
    for (w, h) in [(550, 550), (250, 250), (700, 1000), (630, 550)] {
        let fits = hw <= w && hh <= h;
        for v in [w - hw, h - hh, w - hw, h - hh] {
            o.extend((if fits { v } else { -1 }).to_le_bytes());
        }
    }
    for t in p.threads.iter().take(n) {
        // Janome palette indices are not known; reuse an index the file supplied.
        let idx: u32 = t.code.parse().unwrap_or(1);
        o.extend(idx.to_le_bytes());
    }
    for _ in 0..n {
        o.extend(0x0Du32.to_le_bytes());
    }
    o.extend(body);
    o
}

// ---------------------------------------------------------------- SVG

fn write_svg(p: &Pattern) -> String {
    let b = p.bounds();
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.1}mm\" height=\"{:.1}mm\" viewBox=\"{} {} {} {}\">\n",
        b.width() / 10.0, b.height() / 10.0, b.min_x, b.min_y, b.width().max(1.0), b.height().max(1.0)
    );
    let ci = p.color_indices();
    let mut path = String::new();
    let mut cur = usize::MAX;
    let flush = |s: &mut String, path: &mut String, c: usize| {
        if !path.is_empty() {
            let rgb = p.threads.get(c).map_or([0, 0, 0], |t| t.rgb);
            s.push_str(&format!(
                "<path fill=\"none\" stroke=\"{}\" stroke-width=\"3\" stroke-linecap=\"round\" d=\"{}\"/>\n",
                threads::to_hex(rgb), path.trim()
            ));
            path.clear();
        }
    };
    let mut prev: Option<(f64, f64)> = None;
    for (i, st) in p.stitches.iter().enumerate() {
        if ci[i] != cur {
            flush(&mut s, &mut path, cur);
            cur = ci[i];
            if let Some((x, y)) = prev {
                path.push_str(&format!("M{x} {y} "));
            }
        }
        match st.cmd {
            Cmd::Stitch => {
                if path.is_empty() {
                    let (x, y) = prev.unwrap_or((st.x, st.y));
                    path.push_str(&format!("M{x} {y} "));
                }
                path.push_str(&format!("L{} {} ", st.x, st.y));
            }
            Cmd::Jump => path.push_str(&format!("M{} {} ", st.x, st.y)),
            _ => {}
        }
        prev = Some((st.x, st.y));
    }
    flush(&mut s, &mut path, cur);
    s.push_str("</svg>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Pattern {
        let mut p = Pattern { label: "test".into(), ..Default::default() };
        p.push(0.0, 0.0, Cmd::Jump);
        for i in 0..50 {
            p.push((i * 7 % 40) as f64, (i * 3) as f64, Cmd::Stitch);
        }
        p.trim();
        p.push(300.0, -250.0, Cmd::Jump);
        p.color_change();
        for i in 0..30 {
            p.push(300.0 + (i * 11 % 60) as f64, -250.0 + i as f64 * 2.0, Cmd::Stitch);
        }
        p.threads = vec![threads::pec_thread(5), threads::pec_thread(13)];
        p
    }

    fn sewn(p: &Pattern) -> Vec<(i32, i32)> {
        p.stitches.iter().filter(|s| s.cmd == Cmd::Stitch).map(|s| (s.x.round() as i32, s.y.round() as i32)).collect()
    }

    #[test]
    fn roundtrip_all_formats() {
        let p = sample();
        let dir = std::env::temp_dir().join(format!("stitchery-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for e in READ_EXTS {
            let f = dir.join(format!("t.{e}"));
            write(&f, &p).unwrap();
            let q = read(&f).unwrap();
            let (a, b) = (sewn(&p), sewn(&q));
            // PEC stores positions relative to the top-left corner.
            let off = (b[0].0 - a[0].0, b[0].1 - a[0].1);
            let b: Vec<_> = b.iter().map(|(x, y)| (x - off.0, y - off.1)).collect();
            assert_eq!(a, b, "{e} stitches");
            assert_eq!(q.color_count(), 2, "{e} colors");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dst_ternary_covers_range() {
        for v in -121..=121 {
            let t = ternary(v);
            assert_eq!(t[0] * 81 + t[1] * 27 + t[2] * 9 + t[3] * 3 + t[4], v);
        }
    }
}
