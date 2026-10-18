# Stitchery

A native Linux embroidery design viewer, editor and converter written in Rust,
modeled on the core (free "Express"/viewer) tier of Embrilliance.

## Run

    cargo run --release -- [FILE...]          # GUI; multiple files are merged
    stitchery info design.pes
    stitchery convert design.pes design.dst

## Features

- Read DST, EXP, PEC, PES, JEF; write those plus SVG
- Zoom/pan canvas, hoop overlay, jump display, stitch-out simulation
- Recolor threads, hide colors, remove hidden colors
- Resize, rotate, mirror, center, merge designs, undo/redo, drag and drop
- Thread charts: built-in Brother PEC palette plus CSV charts
  (`code,name,#RRGGBB` per line) with nearest-color matching

## Limitations

- Resizing is geometric: stitch count stays the same, so density changes.
- PES is written as version 1 (PES header + PEC). PES colors are read from the
  PEC palette, so custom RGB colors in newer PES files come back approximated.
- JEF thread colors use palette indices whose Janome RGB table is not included;
  colors read from JEF are placeholders.
- Embrilliance `.bt` thread charts, `.bf` fonts and `.be`/`.bd` working files
  are proprietary and encrypted, so they are not read.
- Not implemented: lettering, appliqué, digitizing (StitchArtist), AlphaTricks,
  Density Repair and other paid modules, along with VP3, HUS, XXX and SEW formats.
