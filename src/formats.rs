//! Machine embroidery file formats, implemented from public format notes.
//!
//! | Format | Read | Write |
//! |--------|------|-------|
//! | DST (Tajima)       | yes | yes |
//! | EXP (Melco/Bernina)| yes | yes |
//! | PEC (Brother)      | yes | yes |
//! | PES (Brother)      | yes (PEC section) | yes (v1 header + PEC) |
//! | JEF (Janome)       | yes | yes |
//! | VP3 (Pfaff/Husqvarna) | yes | yes |
//! | XXX (Singer)       | yes | yes |
//! | SEW (Janome/Elna)  | yes | no  |
//! | HUS (Husqvarna)    | yes | no  |
//! | SVG                | no  | yes |
//!
//! Byte layouts for VP3, XXX, SEW, HUS and PES v6 follow pyembroidery (MIT).

use crate::pattern::{Cmd, Pattern, Thread};
use crate::palettes::{HUS_PALETTE, JEF_PALETTE, SEW_PALETTE};
use crate::threads;
use anyhow::{bail, Context, Result};
use std::path::Path;

pub const READ_EXTS: &[&str] = &["dst", "exp", "pec", "pes", "jef", "vp3", "xxx", "sew", "hus"];
pub const WRITE_EXTS: &[&str] = &["dst", "exp", "pec", "pes", "jef", "vp3", "xxx", "svg"];

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
        "vp3" => read_vp3(&data)?,
        "xxx" => read_xxx(&data)?,
        "sew" => read_sew(&data)?,
        "hus" => read_hus(&data)?,
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
        "vp3" => write_vp3(&p),
        "xxx" => write_xxx(&p),
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

fn pec_colors(p: &Pattern) -> Vec<u8> {
    p.threads.iter().take(p.color_count()).map(|t| {
        // Keep an exact palette index when the thread came from one.
        match t.code.parse::<u8>() {
            Ok(i) if (1..65).contains(&i) && threads::PEC_PALETTE[i as usize].0 == t.rgb => i,
            _ => threads::nearest_pec(t.rgb),
        }
    }).collect()
}

fn pec_block(p: &Pattern) -> Vec<u8> {
    let colors = pec_colors(p);
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
    let mut p = read_pec(d, pec)?;
    // Version 5+ headers carry the real thread colors; prefer them.
    let rgb = pes_threads(d).unwrap_or_default();
    if !rgb.is_empty() && rgb.len() >= p.color_count() {
        p.threads = rgb;
    }
    Ok(p)
}

/// Thread list from a PES v5-v9 header, or None when absent/unparseable.
fn pes_threads(d: &[u8]) -> Option<Vec<Thread>> {
    let version = std::str::from_utf8(d.get(4..8)?).ok()?;
    // (skip before image string, skip after it) following the metadata strings.
    let (a, b) = match version {
        "0050" | "0055" | "0056" => (24, 24),
        "0060" | "0070" => (36, 24),
        "0080" => (38, 26),
        "0090" => (0, 0),
        _ => return None,
    };
    let mut c = Cursor::new(d, 16);
    let string = |c: &mut Cursor| -> Option<String> {
        let n = c.u8()? as usize;
        let s = d.get(c.i..c.i + n)?;
        c.i += n;
        Some(String::from_utf8_lossy(s).into_owned())
    };
    for _ in 0..5 {
        string(&mut c)?;
    }
    if version == "0090" {
        c.i += 14;
        string(&mut c)?;
        c.i += 30;
        string(&mut c)?;
        c.i += 34;
    } else {
        c.i += a;
        string(&mut c)?;
        c.i += b;
    }
    let u16le = |c: &mut Cursor| Some(c.u8()? as u16 | (c.u8()? as u16) << 8);
    // Programmable fills, motifs and feather patterns precede the threads.
    for _ in 0..3 {
        if u16le(&mut c)? != 0 {
            return None;
        }
    }
    let n = u16le(&mut c)?;
    let mut out = Vec::new();
    for _ in 0..n {
        let code = string(&mut c)?;
        let rgb = [c.u8()?, c.u8()?, c.u8()?];
        c.i += 5;
        let name = string(&mut c)?;
        let _brand = string(&mut c)?;
        let _chart = string(&mut c)?;
        out.push(Thread { rgb, name: if name.is_empty() { threads::to_hex(rgb) } else { name }, code });
    }
    Some(out)
}

fn pes_str8(o: &mut Vec<u8>, s: &str) {
    let b = &s.as_bytes()[..s.len().min(255)];
    o.push(b.len() as u8);
    o.extend(b);
}

fn pes_str16(o: &mut Vec<u8>, s: &str) {
    o.extend((s.len() as u16).to_le_bytes());
    o.extend(s.as_bytes());
}

fn u16s(o: &mut Vec<u8>, vals: &[i32]) {
    for v in vals {
        o.extend((*v as u16).to_le_bytes());
    }
}

fn f32s(o: &mut Vec<u8>, vals: &[f32]) {
    for v in vals {
        o.extend(v.to_le_bytes());
    }
}

/// Writes PES version 6: a full PES section with RGB threads and stitch
/// segments, followed by the PEC section and the v6 color addendum.
fn write_pes(p: &Pattern) -> Vec<u8> {
    let n = p.color_count();
    let mut o = b"#PES0060".to_vec();
    let pec_at = o.len();
    o.extend([0; 4]);

    u16s(&mut o, &[1]);
    o.extend(b"02");
    pes_str8(&mut o, &p.label);
    o.extend([0; 4]); // category, author, keywords, comments
    u16s(&mut o, &[0, 0, 0x64, 0x64, 0, 0xC8, 0xC8, 0x64, 0x64, 0x64, 0x07, 0x13, 1, 1, 0, 100, 1, 0]);
    o.push(0); // image file name
    f32s(&mut o, &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    u16s(&mut o, &[0, 0, 0, n as i32]);
    for t in &p.threads[..n] {
        pes_str8(&mut o, &t.code);
        o.extend(t.rgb);
        o.push(0);
        o.extend(0xAu32.to_le_bytes()); // custom color
        pes_str8(&mut o, &t.name);
        pes_str8(&mut o, "");
        pes_str8(&mut o, "");
    }

    let empty = p.stitch_count() == 0;
    u16s(&mut o, &[if empty { 0 } else { 1 }, if empty { 0 } else { 0xFFFF }, 0]);
    if !empty {
        let colorlog = pes_blocks(&mut o, p);
        o.extend([0; 8]);
        for i in 0..colorlog {
            o.extend((i as u32).to_le_bytes());
            o.extend(0u32.to_le_bytes());
        }
    }

    let pec = o.len() as u32;
    o[pec_at..pec_at + 4].copy_from_slice(&pec.to_le_bytes());
    o.extend(pec_block(p));

    let colors = pec_colors(p);
    o.push((colors.len() - 1) as u8);
    o.extend(&colors);
    o.resize(o.len() + 128 - (colors.len() + 1), 0x20);
    for _ in 0..n {
        o.extend([0; 0x90]);
    }
    for t in &p.threads[..n] {
        o.extend([t.rgb[2], t.rgb[1], t.rgb[0]]);
    }
    o.extend([0, 0]);
    o
}

/// CEmbOne + CSewSeg blocks; returns the number of color-log entries.
fn pes_blocks(o: &mut Vec<u8>, p: &Pattern) -> usize {
    let b = p.bounds();
    let (w, h) = (b.width() as f32, b.height() as f32);
    pes_str16(o, "CEmbOne");
    u16s(o, &[0; 8]);
    let tx = 350.0 + 1300.0 / 2.0 - w / 2.0;
    let ty = 100.0 + h + 1800.0 / 2.0 - h / 2.0;
    f32s(o, &[1.0, 0.0, 0.0, 1.0, tx, ty]);
    u16s(o, &[1, 0, 0, w as i32, h as i32]);
    o.extend([0; 8]);
    let sections_at = o.len();
    o.extend([0, 0]);
    u16s(o, &[0xFFFF, 0]);
    pes_str16(o, "CSewSeg");

    let (ax, ay) = (b.min_x, b.max_y);
    let st = &p.stitches;
    let mut color = 0usize;
    let (mut sx, mut sy) = (0.0, 0.0);
    let mut sections = 0;
    let mut log: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < st.len() {
        let cmd = st[i].cmd;
        let mut j = i;
        while j < st.len() && st[j].cmd == cmd {
            j += 1;
        }
        let run = &st[i..j];
        i = j;
        let (seg, flag): (Vec<(f64, f64)>, i32) = match cmd {
            Cmd::Jump => {
                let last = run.last().unwrap();
                (vec![(sx, sy), (last.x, last.y)], 1)
            }
            Cmd::Stitch => {
                let last = run.last().unwrap();
                (sx, sy) = (last.x, last.y);
                (run.iter().map(|s| (s.x, s.y)).collect(), 0)
            }
            Cmd::ColorChange => {
                color += run.len();
                continue;
            }
            Cmd::Trim => continue,
        };
        if sections > 0 {
            u16s(o, &[0x8003]);
        }
        if log.last().is_none_or(|l| l.1 != color) {
            log.push((sections, color));
        }
        u16s(o, &[flag, color as i32, seg.len() as i32]);
        for (x, y) in seg {
            u16s(o, &[(x - ax) as i32, (y - ay) as i32]);
        }
        sections += 1;
    }
    u16s(o, &[log.len() as i32]);
    for (s, c) in &log {
        u16s(o, &[*s as i32, *c as i32]);
    }
    o[sections_at..sections_at + 2].copy_from_slice(&(sections as u16).to_le_bytes());
    u16s(o, &[0, 0]);
    log.len()
}

// ---------------------------------------------------------------- JEF

fn read_jef(d: &[u8]) -> Result<Pattern> {
    let start = le32(d, 0)? as usize;
    let ncolors = le32(d, 24)? as usize;
    let mut p = Pattern::default();
    for i in 0..ncolors.min(256) {
        let idx = le32(d, 116 + 4 * i).unwrap_or(0) as usize;
        p.threads.push(palette_thread(&JEF_PALETTE, idx));
    }
    janome_stitches(&mut p, d, start, true);
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
        let idx = threads::nearest_index(JEF_PALETTE.iter().map(|e| &e.0), 1, t.rgb) as u32;
        o.extend(idx.to_le_bytes());
    }
    for _ in 0..n {
        o.extend(0x0Du32.to_le_bytes());
    }
    o.extend(body);
    o
}

fn palette_thread(pal: &[([u8; 3], &str, &str)], idx: usize) -> Thread {
    let (rgb, name, code) = pal.get(idx).copied().unwrap_or(pal[0]);
    Thread { rgb, name: name.to_string(), code: code.to_string() }
}

/// Reads the shared Janome-style stitch stream (JEF body, SEW).
/// In JEF `80 10` ends the design; in SEW it is a stitch.
fn janome_stitches(p: &mut Pattern, d: &[u8], start: usize, jef: bool) {
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
        if b == 0x10 && (jef || d.len() - c.i < 2) {
            break;
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
            0x10 => {
                x += sb(dx);
                y -= sb(dy);
                p.push(x, y, Cmd::Stitch);
            }
            _ => break,
        }
    }
}

// ---------------------------------------------------------------- SEW

fn read_sew(d: &[u8]) -> Result<Pattern> {
    let n = u16::from_le_bytes(d.get(0..2).context("SEW truncated")?.try_into().unwrap()) as usize;
    let mut p = Pattern::default();
    for i in 0..n {
        let at = 2 + 2 * i;
        let idx = u16::from_le_bytes(d.get(at..at + 2).context("SEW truncated")?.try_into().unwrap()) as usize;
        p.threads.push(palette_thread(&SEW_PALETTE, idx % SEW_PALETTE.len()));
    }
    if d.len() < 0x1D78 {
        bail!("SEW file too short");
    }
    janome_stitches(&mut p, d, 0x1D78, false);
    Ok(p)
}

// ---------------------------------------------------------------- XXX

fn read_xxx(d: &[u8]) -> Result<Pattern> {
    if d.len() < 0x100 {
        bail!("XXX file too short");
    }
    let ncolors = u16::from_le_bytes([d[0x27], d[0x28]]) as usize;
    let mut p = Pattern::default();
    let mut c = Cursor::new(d, 0x100);
    let (mut x, mut y) = (0.0, 0.0);
    let sb = |b: u8| b as i8 as f64;
    loop {
        let Some(b1) = c.u8() else { break };
        if b1 == 0x7D || b1 == 0x7E {
            let (Some(a), Some(b), Some(e), Some(f)) = (c.u8(), c.u8(), c.u8(), c.u8()) else { break };
            x += i16::from_le_bytes([a, b]) as f64;
            y -= i16::from_le_bytes([e, f]) as f64;
            p.push(x, y, Cmd::Jump);
            continue;
        }
        let Some(b2) = c.u8() else { break };
        if b1 != 0x7F {
            x += sb(b1);
            y -= sb(b2);
            p.push(x, y, Cmd::Stitch);
            continue;
        }
        let (Some(b3), Some(b4)) = (c.u8(), c.u8()) else { break };
        match b2 {
            0x01 => {
                x += sb(b3);
                y -= sb(b4);
                p.push(x, y, Cmd::Jump);
            }
            0x03 => {
                p.trim();
                if b3 != 0 || b4 != 0 {
                    x += sb(b3);
                    y -= sb(b4);
                    p.push(x, y, Cmd::Jump);
                }
            }
            0x08 | 0x0A..=0x17 => p.color_change(),
            _ => break,
        }
    }
    c.i += 2;
    for _ in 0..ncolors {
        let (Some(_), Some(r), Some(g), Some(b)) = (c.u8(), c.u8(), c.u8(), c.u8()) else { break };
        p.threads.push(Thread::new([r, g, b], threads::to_hex([r, g, b])));
    }
    Ok(p)
}

fn write_xxx(p: &Pattern) -> Vec<u8> {
    let n = p.color_count();
    let rel = p.relative(123);
    let b = p.bounds();
    let (lx, ly) = p.pos();
    let mut o = vec![0u8; 0x17];
    o.extend((rel.len() as u32).to_le_bytes());
    o.extend([0; 0x0C]);
    o.extend((n as u32).to_le_bytes());
    u16s(&mut o, &[0, b.width() as i32, b.height() as i32, lx as i32, -ly as i32, -b.min_x as i32, b.max_y as i32]);
    o.extend([0; 0x42 + 4 + 0x73]);
    u16s(&mut o, &[0x20]);
    o.extend([0; 8]);
    let end_at = o.len();
    o.extend([0; 4]);
    for (dx, dy, cmd) in rel {
        let (bx, by) = (dx as i8 as u8, (-dy) as i8 as u8);
        match cmd {
            Cmd::Stitch => o.extend([bx, by]),
            Cmd::Jump => o.extend([0x7F, 0x01, bx, by]),
            Cmd::Trim => o.extend([0x7F, 0x03, 0, 0]),
            Cmd::ColorChange => o.extend([0x7F, 0x08, 0, 0]),
        }
    }
    let end = o.len() as u32;
    o[end_at..end_at + 4].copy_from_slice(&end.to_le_bytes());
    o.extend([0x7F, 0x7F, 0x02, 0x14, 0, 0]);
    for t in &p.threads[..n] {
        o.extend([0, t.rgb[0], t.rgb[1], t.rgb[2]]);
    }
    for _ in n..21 {
        o.extend([0; 4]);
    }
    o.extend(0xFFFFFF00u32.to_le_bytes());
    o.extend([0, 1]);
    o
}

// ---------------------------------------------------------------- VP3

fn be32(d: &[u8], at: usize) -> Result<i32> {
    let b = d.get(at..at + 4).context("VP3 truncated")?;
    Ok(i32::from_be_bytes(b.try_into().unwrap()))
}

fn be16(d: &[u8], at: usize) -> Result<usize> {
    let b = d.get(at..at + 2).context("VP3 truncated")?;
    Ok(u16::from_be_bytes(b.try_into().unwrap()) as usize)
}

fn read_vp3(d: &[u8]) -> Result<Pattern> {
    if !d.starts_with(b"%vsm%") {
        bail!("not a VP3 file");
    }
    let skip_str = |at: usize| -> Result<usize> { Ok(at + 2 + be16(d, at)?) };
    let mut i = skip_str(6)? + 7;
    i = skip_str(i)? + 32;
    let cx = be32(d, i)? as f64 / 100.0;
    let cy = -(be32(d, i + 4)? as f64) / 100.0;
    i = skip_str(i + 8 + 27)? + 24;
    i = skip_str(i)?;
    let blocks = be16(d, i)?;
    i += 2;
    let mut p = Pattern::default();
    for b in 0..blocks {
        let end = i + 7 + be32(d, i + 3)? as usize;
        let sx = be32(d, i + 7)? as f64 / 100.0 + cx;
        let sy = -(be32(d, i + 11)? as f64) / 100.0 + cy;
        i += 15;
        if b > 0 {
            p.color_change();
        }
        p.push(sx, sy, Cmd::Jump);
        // Thread: color count, transition, (rgb24, parts, length16) per color, type, weight, 3 strings.
        let ncol = *d.get(i).context("VP3 truncated")? as usize;
        let mut rgb = [0, 0, 0];
        i += 2;
        for _ in 0..ncol {
            rgb.copy_from_slice(d.get(i..i + 3).context("VP3 truncated")?);
            i += 6;
        }
        i += 2;
        let mut strs = Vec::new();
        for _ in 0..3 {
            let n = be16(d, i)?;
            strs.push(String::from_utf8_lossy(d.get(i + 2..i + 2 + n).context("VP3 truncated")?).into_owned());
            i += 2 + n;
        }
        let name = if strs[1].is_empty() { threads::to_hex(rgb) } else { strs[1].clone() };
        p.threads.push(Thread { rgb, name, code: strs[0].clone() });
        i += 15 + 3;
        let (mut x, mut y) = (sx, sy);
        let data = d.get(i..end.min(d.len())).context("VP3 truncated")?;
        let mut k = 0;
        while k + 1 < data.len() {
            let (a, b) = (data[k], data[k + 1]);
            k += 2;
            if a != 0x80 {
                x += a as i8 as f64;
                y += b as i8 as f64;
                p.push(x, y, Cmd::Stitch);
                continue;
            }
            match b {
                0x01 if k + 4 <= data.len() => {
                    let dx = i16::from_be_bytes([data[k], data[k + 1]]) as f64;
                    let dy = i16::from_be_bytes([data[k + 2], data[k + 3]]) as f64;
                    x += dx;
                    y += dy;
                    k += 6;
                    // VP3 has no jump command; treat moves longer than any
                    // machine stitch (12.1 mm) as frame moves.
                    let cmd = if dx.abs() > 121.0 || dy.abs() > 121.0 { Cmd::Jump } else { Cmd::Stitch };
                    p.push(x, y, cmd);
                }
                0x03 => p.trim(),
                _ => {}
            }
        }
        i = end;
    }
    Ok(p)
}

fn vp3_str8(o: &mut Vec<u8>, s: &str) {
    o.extend((s.len() as u16).to_be_bytes());
    o.extend(s.as_bytes());
}

fn vp3_str16(o: &mut Vec<u8>, s: &str) {
    let u: Vec<u16> = s.encode_utf16().collect();
    o.extend(((u.len() * 2) as u16).to_be_bytes());
    for c in u {
        o.extend(c.to_be_bytes());
    }
}

fn i32s_be(o: &mut Vec<u8>, vals: &[i32]) {
    for v in vals {
        o.extend(v.to_be_bytes());
    }
}

/// Backfills a big-endian length placeholder at `at` with the bytes after it.
fn patch_len(o: &mut [u8], at: usize) {
    let n = (o.len() - at - 4) as u32;
    o[at..at + 4].copy_from_slice(&n.to_be_bytes());
}

fn write_vp3(p: &Pattern) -> Vec<u8> {
    let b = p.bounds();
    let n = p.color_count();
    let mut o = b"%vsm%\0".to_vec();
    vp3_str16(&mut o, "Produced by     Software Ltd");
    o.extend([0, 2, 0]);
    let file_len = o.len();
    o.extend([0; 4]);
    vp3_str16(&mut o, "");
    i32s_be(&mut o, &[(b.max_x * 100.0) as i32, (b.min_y * -100.0) as i32, (b.min_x * 100.0) as i32, (b.max_y * -100.0) as i32]);
    i32s_be(&mut o, &[p.stitches.len() as i32]);
    o.extend([0, n as u8, 12, 0, 1]);

    o.extend([0, 3, 0]);
    let design_len = o.len();
    o.extend([0; 4]);
    let (hw, hh) = ((b.width() / 2.0) as i32, (b.height() / 2.0) as i32);
    let (cx, cy) = ((b.max_x - b.width() / 2.0) as i32, (b.max_y - b.height() / 2.0) as i32);
    i32s_be(&mut o, &[cx * 100, cy * -100]);
    o.extend([0, 0, 0]);
    i32s_be(&mut o, &[-hw * 100, hw * 100, -hh * 100, hh * 100, b.width() as i32 * 100, b.height() as i32 * 100]);
    vp3_str16(&mut o, "");
    o.extend([0x64, 0x64]);
    i32s_be(&mut o, &[4096, 0, 0, 4096]);
    o.extend(b"xxPP\x01\x00");
    vp3_str16(&mut o, "Produced by     Software Ltd");
    o.extend((n as u16).to_be_bytes());

    // Split the stitch stream into color blocks.
    let mut blocks: Vec<Vec<(i32, i32, Cmd)>> = vec![vec![]];
    for s in &p.stitches {
        if s.cmd == Cmd::ColorChange {
            blocks.push(vec![]);
        } else {
            blocks.last_mut().unwrap().push((s.x.round() as i32, s.y.round() as i32, s.cmd));
        }
    }
    for (bi, (block, t)) in blocks.iter().zip(&p.threads).enumerate() {
        o.extend([0, 5, 0]);
        let block_len = o.len();
        o.extend([0; 4]);
        let first = if bi == 0 { (0, 0) } else { block.first().map_or((0, 0), |s| (s.0, s.1)) };
        let last = block.last().map_or(first, |s| (s.0, s.1));
        i32s_be(&mut o, &[(first.0 - cx) * 100, -(first.1 - cy) * 100]);
        o.extend([1, 0, t.rgb[0], t.rgb[1], t.rgb[2], 0, 0, 0, 5, 0x28]);
        vp3_str8(&mut o, &t.code);
        vp3_str8(&mut o, &t.name);
        vp3_str8(&mut o, "");
        i32s_be(&mut o, &[(last.0 - first.0) * 100, -(last.1 - first.1) * 100]);
        o.extend([0, 1, 0]);
        let stitch_len = o.len();
        o.extend([0; 4]);
        o.extend([0x0A, 0xF6, 0]);
        let (mut lx, mut ly) = first;
        for &(x, y, cmd) in block {
            let (dx, dy) = (x - lx, y - ly);
            match cmd {
                Cmd::Trim => o.extend([0x80, 0x03]),
                Cmd::Jump => {
                    if dx != 0 || dy != 0 {
                        o.extend([0x80, 0x01]);
                        o.extend((dx as i16).to_be_bytes());
                        o.extend((dy as i16).to_be_bytes());
                        o.extend([0x80, 0x02]);
                    }
                    (lx, ly) = (x, y);
                }
                Cmd::Stitch => {
                    let steps = ((dx.abs().max(dy.abs()) + 126) / 127).max(1);
                    let (mut px, mut py) = (0, 0);
                    for k in 1..=steps {
                        let (nx, ny) = (dx * k / steps, dy * k / steps);
                        o.extend([(nx - px) as i8 as u8, (ny - py) as i8 as u8]);
                        (px, py) = (nx, ny);
                    }
                    (lx, ly) = (x, y);
                }
                Cmd::ColorChange => {}
            }
        }
        patch_len(&mut o, stitch_len);
        o.push(0);
        patch_len(&mut o, block_len);
    }
    patch_len(&mut o, design_len);
    patch_len(&mut o, file_len);
    o
}

// ---------------------------------------------------------------- HUS

fn read_hus(d: &[u8]) -> Result<Pattern> {
    let n = le32(d, 4)? as usize;
    let ncolors = le32(d, 8)? as usize;
    let (cmd_at, x_at, y_at) = (le32(d, 20)? as usize, le32(d, 24)? as usize, le32(d, 28)? as usize);
    let mut p = Pattern::default();
    for i in 0..ncolors {
        let at = 42 + 2 * i;
        let idx = u16::from_le_bytes(d.get(at..at + 2).context("HUS truncated")?.try_into().unwrap()) as usize;
        p.threads.push(palette_thread(&HUS_PALETTE, idx % HUS_PALETTE.len()));
    }
    let section = |a: usize, b: usize| d.get(a..b).context("HUS offsets out of range");
    let cmds = expand(section(cmd_at, x_at)?, n);
    let xs = expand(section(x_at, y_at)?, n);
    let ys = expand(section(y_at, d.len())?, n);
    let (mut x, mut y) = (0.0, 0.0);
    for i in 0..cmds.len().min(xs.len()).min(ys.len()) {
        let (dx, dy) = (xs[i] as i8 as f64, -(ys[i] as i8 as f64));
        match cmds[i] {
            0x80 => {
                x += dx;
                y += dy;
                p.push(x, y, Cmd::Stitch);
            }
            0x81 => {
                x += dx;
                y += dy;
                p.push(x, y, Cmd::Jump);
            }
            0x84 => {
                p.color_change();
                if dx != 0.0 || dy != 0.0 {
                    x += dx;
                    y += dy;
                    p.push(x, y, Cmd::Jump);
                }
            }
            0x88 => {
                if dx != 0.0 || dy != 0.0 {
                    x += dx;
                    y += dy;
                    p.push(x, y, Cmd::Jump);
                }
                p.trim();
            }
            _ => break,
        }
    }
    Ok(p)
}

/// Huffman table built from code lengths; lowest index wins ties.
struct Huffman {
    lengths: Vec<u32>,
    table: Vec<usize>,
    width: u32,
    default: usize,
}

impl Huffman {
    fn constant(v: usize) -> Self {
        Huffman { lengths: vec![], table: vec![], width: 0, default: v }
    }
    fn new(lengths: Vec<u32>) -> Self {
        let width = lengths.iter().copied().max().unwrap_or(0);
        let mut table = Vec::new();
        for bits in 1..=width {
            let size = 1usize << (width - bits);
            for (i, &l) in lengths.iter().enumerate() {
                if l == bits {
                    table.extend(std::iter::repeat_n(i, size));
                }
            }
        }
        Huffman { lengths, table, width, default: 0 }
    }
    /// (value, bit length) for the next 16 bits.
    fn lookup(&self, bits16: u32) -> (usize, u32) {
        if self.width == 0 {
            return (self.default, 0);
        }
        let v = self.table.get((bits16 >> (16 - self.width)) as usize).copied().unwrap_or(0);
        (v, self.lengths[v])
    }
}

struct BitReader<'a> {
    d: &'a [u8],
    pos: usize,
}

impl BitReader<'_> {
    fn peek(&self, n: u32) -> u32 {
        let mut v = 0u64;
        for k in 0..4 {
            v = (v << 8) | *self.d.get(self.pos / 8 + k).unwrap_or(&0) as u64;
        }
        ((v >> (32 - (self.pos % 8) as u32 - n)) & ((1u64 << n) - 1)) as u32
    }
    fn pop(&mut self, n: u32) -> u32 {
        let v = self.peek(n);
        self.pos += n as usize;
        v
    }
    fn varlen(&mut self) -> u32 {
        let mut m = self.pop(3);
        if m == 7 {
            for _ in 0..13 {
                if self.pop(1) == 1 { m += 1 } else { break }
            }
        }
        m
    }
}

/// Decompressor for the LZH-style streams in HUS files.
fn expand(data: &[u8], limit: usize) -> Vec<u8> {
    let mut r = BitReader { d: data, pos: 0 };
    let mut out: Vec<u8> = Vec::new();
    let mut remaining: i64 = -1;
    let mut chars = Huffman::constant(0);
    let mut dist = Huffman::constant(0);
    while r.pos < data.len() * 8 && out.len() <= limit {
        if remaining <= 0 {
            remaining = r.pop(16) as i64;
            // Code-length huffman.
            let count = r.pop(5) as usize;
            let lens = if count == 0 {
                Huffman::constant(r.pop(5) as usize)
            } else {
                let mut l = vec![0; count];
                let mut i = 0;
                while i < count {
                    if i == 3 {
                        i += r.pop(2) as usize;
                    }
                    if i < count {
                        l[i] = r.varlen();
                    }
                    i += 1;
                }
                Huffman::new(l)
            };
            let count = r.pop(9) as usize;
            chars = if count == 0 {
                Huffman::constant(r.pop(9) as usize)
            } else {
                let mut l = vec![0; count];
                let mut i = 0;
                while i < count {
                    let (c, n) = lens.lookup(r.peek(16));
                    r.pos += n as usize;
                    match c {
                        0 => i += 1,
                        1 => i += 3 + r.pop(4) as usize,
                        2 => i += 20 + r.pop(9) as usize,
                        _ => {
                            l[i] = c as u32 - 2;
                            i += 1;
                        }
                    }
                }
                Huffman::new(l)
            };
            let count = r.pop(5) as usize;
            dist = if count == 0 {
                Huffman::constant(r.pop(5) as usize)
            } else {
                Huffman::new((0..count).map(|_| r.varlen()).collect())
            };
        }
        remaining -= 1;
        let (c, n) = chars.lookup(r.peek(16));
        r.pos += n as usize;
        if c <= 255 {
            out.push(c as u8);
        } else if c == 510 {
            break;
        } else {
            let len = c - 253;
            let (h, n) = dist.lookup(r.peek(16));
            r.pos += n as usize;
            let back = if h == 0 { 0 } else { (1usize << (h - 1)) + r.pop(h as u32 - 1) as usize } + 1;
            let Some(start) = out.len().checked_sub(back) else { break };
            for k in start..start + len {
                out.push(out[k]);
            }
        }
    }
    out
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
        for e in WRITE_EXTS.iter().filter(|e| READ_EXTS.contains(e)) {
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
