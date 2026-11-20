# M1 fixture assets

`checker.png` is an original 2×2, 8-bit RGBA PNG generated from the explicit
pixel matrix in `png-source.json`. Its top row is opaque red/green and its bottom
row is opaque blue/transparent. `truncated.png` deliberately ends inside IHDR and
must fail decoding. Both use the repository MIT license.

The Noto font files are unmodified upstream files pinned to the exact Google Fonts
commit and SHA-256 in `font-sources.json`. Each font family includes its original
SIL Open Font License 1.1 in its `OFL.txt`; these assets are not relicensed as MIT.
Use width 100 and weight 400, OpenType font functions, fixed script/direction/language,
and the recorded shaping scale for reference glyph runs. No system font fallback
is allowed in deterministic M1 fixtures.
