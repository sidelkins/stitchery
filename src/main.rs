mod app;
mod formats;
mod lettering;
mod palettes;
mod pattern;
mod threads;

use anyhow::{bail, Result};
use std::path::PathBuf;

const USAGE: &str = "\
usage:
  stitchery [FILE...]              open the editor (multiple files are merged)
  stitchery info FILE...           print design statistics
  stitchery convert IN OUT         convert between formats (by extension)
  stitchery text FONT TEXT OUT [HEIGHT_MM] [satin|fill|outline]
                                   stitch lettering; FONT is a file or font name

read:  dst exp pec pes jef vp3 xxx sew hus
write: dst exp pec pes jef vp3 xxx svg";

fn info(path: &PathBuf) -> Result<()> {
    let p = formats::read(path)?;
    let b = p.bounds();
    println!("{}", path.display());
    println!("  label:    {}", p.label);
    println!("  size:     {:.1} x {:.1} mm", b.width() / 10.0, b.height() / 10.0);
    println!("  stitches: {}  jumps: {}  trims: {}", p.stitch_count(), p.jump_count(), p.trim_count());
    println!("  colors:   {}", p.color_count());
    for (i, t) in p.threads.iter().take(p.color_count()).enumerate() {
        println!("    {:2}. {} {}", i + 1, threads::to_hex(t.rgb), t.name);
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("-h" | "--help") => println!("{USAGE}"),
        Some("info") => {
            if args.len() < 2 {
                bail!("{USAGE}");
            }
            for a in &args[1..] {
                info(&PathBuf::from(a))?;
            }
        }
        Some("convert") => {
            let [_, input, output] = args.as_slice() else { bail!("{USAGE}") };
            let p = formats::read(input.as_ref())?;
            formats::write(output.as_ref(), &p)?;
            println!("wrote {output}");
        }
        Some("text") => {
            let [_, font, text, out, rest @ ..] = args.as_slice() else { bail!("{USAGE}") };
            let path = if std::path::Path::new(font).is_file() {
                PathBuf::from(font)
            } else {
                let fonts = lettering::system_fonts();
                let f = fonts.iter().find(|f| f.name.eq_ignore_ascii_case(font))
                    .or_else(|| fonts.iter().find(|f| f.name.to_lowercase().contains(&font.to_lowercase())));
                match f {
                    Some(f) => f.path.clone(),
                    None => bail!("no installed font matches '{font}'"),
                }
            };
            let mut spec = lettering::TextSpec { text: text.replace("\\n", "\n"), font: path, ..Default::default() };
            if let Some(h) = rest.first() {
                spec.height_mm = h.parse()?;
            }
            spec.style = match rest.get(1).map(String::as_str) {
                None | Some("satin") => lettering::FillStyle::Satin,
                Some("fill") => lettering::FillStyle::Tatami,
                Some("outline") => lettering::FillStyle::Outline,
                Some(s) => bail!("unknown style '{s}'"),
            };
            let mut p = lettering::render(&spec)?;
            p.center();
            formats::write(out.as_ref(), &p)?;
            let b = p.bounds();
            println!("wrote {out}: {:.1} x {:.1} mm, {} stitches", b.width() / 10.0, b.height() / 10.0, p.stitch_count());
        }
        _ => app::run(args.into_iter().map(PathBuf::from).collect())?,
    }
    Ok(())
}
