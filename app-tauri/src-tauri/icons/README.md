# Icons

The real OctoPad artwork, generated from `linux-app/octopad.svg` (the same
source `linux-app/octopad.png` was exported from — the 512x512 render below is
byte-for-byte identical to it). Nothing here is a Tauri placeholder any more.

Regenerate after an artwork change:

```bash
# 1024x1024 is what the Tauri icon generator wants; it renders every size,
# including the 512 icns entry, from it.
inkscape ../../linux-app/octopad.svg --export-type=png \
  --export-filename=/tmp/octopad-1024.png -w 1024 -h 1024 \
  --export-background-opacity=0
npx tauri icon /tmp/octopad-1024.png
```

Then check the result is real artwork and not a blank file — every one of
these has hundreds of distinct colours and a large standard deviation:

```bash
identify -format '%f %wx%h colors=%k std=%[standard-deviation]\n' *.png
identify icon.ico            # 16/24/32/48/64/256 frames
```

`icon.ico` and `icon.icns` are containers: `identify` cannot read `.icns` here
(no libicns), so verify it by unpacking the PNG payloads — see the Phase 2 notes
in `docs/superpowers/plans/2026-10-19-memory-ui-m3.md`.
