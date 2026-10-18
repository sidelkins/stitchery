mod app;
mod formats;
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
        _ => app::run(args.into_iter().map(PathBuf::from).collect())?,
    }
    Ok(())
}
