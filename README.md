# Stitchery

A native Linux embroidery design viewer, editor and converter written in Rust,
modeled on the core (free "Express"/viewer) tier of Embrilliance.

## Run

    cargo run --release -- [FILE...]          # GUI; multiple files are merged
    stitchery info design.pes
    stitchery convert design.pes design.dst
    stitchery text "Adwaita Sans" "Hello" hello.pes 20 satin   # height mm; satin|fill|outline

## Features

- Read DST, EXP, PEC, PES, JEF, VP3, XXX, SEW, HUS
- Write DST, EXP, PEC, PES (v6, with RGB thread colors), JEF, VP3, XXX, SVG
- Lettering from any installed TrueType/OpenType font (Design ▸ Add lettering):
  satin, tatami fill or outline; underlay, density, angle, letter spacing
- Zoom/pan canvas, hoop overlay, jump display, stitch-out simulation
- Recolor threads, hide colors, remove hidden colors
- Resize, rotate, mirror, center, merge designs, undo/redo, drag and drop
- Thread charts: built-in Brother PEC palette plus CSV charts
  (`code,name,#RRGGBB` per line) with nearest-color matching

## Limitations

- Resizing is geometric: stitch count stays the same, so density changes.
- PES files older than v5 carry only Brother palette colors.
- JEF, SEW and HUS store palette indices, so written colors snap to the nearest
  machine palette entry.
- VP3 has no jump command: long moves (> 12.1 mm) are read as jumps.
- Satin lettering uses one fixed stitch angle with long stitches split at 7 mm,
  so it suits small text best; hand-digitized embroidery fonts angle satin
  along each stroke.
- Embrilliance `.bt` thread charts, `.bf` fonts and `.be`/`.bd` working files
  are proprietary and encrypted, so they are not read.
- Not implemented: appliqué, digitizing (StitchArtist), AlphaTricks, Density
  Repair and other paid modules; HUS and SEW writing.
