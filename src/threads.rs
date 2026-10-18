//! Thread palettes: the fixed Brother PEC palette and user-supplied charts.
//!
//! Embrilliance's own `.bt` charts are encrypted, so charts are imported from
//! plain CSV instead: one thread per line as `code,name,#RRGGBB` (or
//! `code,name,r,g,b`). Lines starting with `#` followed by a non-hex word, or
//! empty lines, are ignored.

use crate::pattern::Thread;
use anyhow::{bail, Context, Result};
use std::path::Path;

/// Colors used when a file carries no color information.
pub const DEFAULT_CYCLE: [[u8; 3]; 12] = [
    [10, 85, 163], [237, 23, 31], [112, 188, 31], [255, 217, 17],
    [145, 54, 151], [0, 135, 119], [254, 158, 50], [0, 0, 0],
    [246, 74, 138], [37, 132, 187], [125, 111, 0], [135, 135, 135],
];

/// Brother PEC thread palette; index 0 is unused by the format.
pub const PEC_PALETTE: [([u8; 3], &str); 65] = [
    ([0, 0, 0], "Unknown"),
    ([14, 31, 124], "Prussian Blue"),
    ([10, 85, 163], "Blue"),
    ([0, 135, 119], "Teal Green"),
    ([75, 107, 175], "Cornflower Blue"),
    ([237, 23, 31], "Red"),
    ([209, 92, 0], "Reddish Brown"),
    ([145, 54, 151], "Magenta"),
    ([228, 154, 203], "Light Lilac"),
    ([145, 95, 172], "Lilac"),
    ([158, 214, 125], "Mint Green"),
    ([232, 169, 0], "Deep Gold"),
    ([254, 186, 53], "Orange"),
    ([255, 255, 0], "Yellow"),
    ([112, 188, 31], "Lime Green"),
    ([186, 152, 0], "Brass"),
    ([168, 168, 168], "Silver"),
    ([125, 111, 0], "Russet Brown"),
    ([255, 255, 179], "Cream Brown"),
    ([79, 85, 86], "Pewter"),
    ([0, 0, 0], "Black"),
    ([11, 61, 145], "Ultramarine"),
    ([119, 1, 118], "Royal Purple"),
    ([41, 49, 51], "Dark Gray"),
    ([42, 19, 1], "Dark Brown"),
    ([246, 74, 138], "Deep Rose"),
    ([178, 118, 36], "Light Brown"),
    ([252, 187, 197], "Salmon Pink"),
    ([254, 55, 15], "Vermilion"),
    ([240, 240, 240], "White"),
    ([106, 28, 138], "Violet"),
    ([168, 221, 196], "Seacrest"),
    ([37, 132, 187], "Sky Blue"),
    ([254, 179, 67], "Pumpkin"),
    ([255, 243, 107], "Cream Yellow"),
    ([208, 166, 96], "Khaki"),
    ([209, 84, 0], "Clay Brown"),
    ([102, 186, 73], "Leaf Green"),
    ([19, 74, 70], "Peacock Blue"),
    ([135, 135, 135], "Gray"),
    ([216, 204, 198], "Warm Gray"),
    ([67, 86, 7], "Dark Olive"),
    ([253, 217, 222], "Linen"),
    ([249, 147, 188], "Pink"),
    ([0, 56, 34], "Deep Green"),
    ([178, 175, 212], "Lavender"),
    ([104, 106, 176], "Wisteria Violet"),
    ([239, 227, 185], "Beige"),
    ([247, 56, 102], "Carmine"),
    ([181, 75, 100], "Amber Red"),
    ([19, 43, 26], "Olive Green"),
    ([199, 1, 86], "Dark Fuchsia"),
    ([254, 158, 50], "Tangerine"),
    ([168, 222, 235], "Light Blue"),
    ([0, 103, 62], "Emerald Green"),
    ([78, 41, 144], "Purple"),
    ([47, 126, 32], "Moss Green"),
    ([255, 204, 204], "Flesh Pink"),
    ([255, 217, 17], "Harvest Gold"),
    ([9, 91, 166], "Electric Blue"),
    ([240, 249, 112], "Lemon Yellow"),
    ([227, 243, 91], "Fresh Green"),
    ([255, 153, 0], "Applique Material"),
    ([255, 240, 141], "Applique Position"),
    ([255, 200, 200], "Applique"),
];

pub fn pec_thread(index: u8) -> Thread {
    let (rgb, name) = PEC_PALETTE.get(index as usize).copied().unwrap_or(PEC_PALETTE[0]);
    Thread { rgb, name: name.to_string(), code: index.to_string() }
}

/// Perceptual-ish distance (weighted RGB, "redmean").
pub fn distance(a: [u8; 3], b: [u8; 3]) -> f64 {
    let rm = (a[0] as f64 + b[0] as f64) / 2.0;
    let dr = a[0] as f64 - b[0] as f64;
    let dg = a[1] as f64 - b[1] as f64;
    let db = a[2] as f64 - b[2] as f64;
    ((2.0 + rm / 256.0) * dr * dr + 4.0 * dg * dg + (2.0 + (255.0 - rm) / 256.0) * db * db).sqrt()
}

/// Index of the palette entry closest to `rgb`, skipping `skip` leading entries.
pub fn nearest_index<'a>(palette: impl Iterator<Item = &'a [u8; 3]>, skip: usize, rgb: [u8; 3]) -> usize {
    palette
        .enumerate()
        .skip(skip)
        .min_by(|a, b| distance(*a.1, rgb).total_cmp(&distance(*b.1, rgb)))
        .map_or(skip, |(i, _)| i)
}

pub fn nearest_pec(rgb: [u8; 3]) -> u8 {
    (1..PEC_PALETTE.len())
        .min_by(|&a, &b| distance(PEC_PALETTE[a].0, rgb).total_cmp(&distance(PEC_PALETTE[b].0, rgb)))
        .unwrap() as u8
}

#[derive(Clone, Debug)]
pub struct Chart {
    pub name: String,
    pub threads: Vec<Thread>,
}

impl Chart {
    pub fn pec() -> Chart {
        Chart { name: "Brother PEC".into(), threads: (1..65).map(pec_thread).collect() }
    }

    pub fn load_csv(path: &Path) -> Result<Chart> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut threads = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || (line.starts_with('#') && parse_hex(line).is_none()) {
                continue;
            }
            let f: Vec<&str> = line.split(',').map(str::trim).collect();
            let rgb = match f.len() {
                3 => parse_hex(f[2]),
                5.. => (|| Some([f[2].parse().ok()?, f[3].parse().ok()?, f[4].parse().ok()?]))(),
                _ => None,
            };
            let Some(rgb) = rgb else {
                // Tolerate a header row; reject anything else.
                if n == 0 {
                    continue;
                }
                bail!("{}:{}: expected code,name,#RRGGBB", path.display(), n + 1);
            };
            threads.push(Thread { rgb, name: f[1].to_string(), code: f[0].to_string() });
        }
        let name = path.file_stem().map_or("Chart".into(), |s| s.to_string_lossy().into_owned());
        Ok(Chart { name, threads })
    }

    pub fn nearest(&self, rgb: [u8; 3]) -> Option<&Thread> {
        self.threads.iter().min_by(|a, b| distance(a.rgb, rgb).total_cmp(&distance(b.rgb, rgb)))
    }
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}
