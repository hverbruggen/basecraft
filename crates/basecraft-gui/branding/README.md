# Basecraft wordmark

Accepted full logo: the soft-gradient lowercase wordmark, with its internal transparency gradient retained
for reuse in study 21 (2026-10-26).
The folded b is emphasised; the compressed JetBrainsMono lettering retains an 18%
interior opacity and gentle 20% edge shading. The b and subtitle have no global
fade. Only the opening-window presentation applies an additional 70% overall
opacity to the complete composition.

`wordmark-dark.svg` is transparent artwork for dark surfaces;
`wordmark-light.svg` is transparent artwork for light surfaces. Both are editable
vector masters with outlined lettering, requiring no installed font.

The opening panel uses `logo-transparent-dark.png` and
`logo-transparent-light.png`: 3060-pixel derivatives of the complete logo-and-
subtitle SVGs. Rendering the whole composition preserves its subtitle line
breaks, spacing and proportions without font-dependent reflow. The PNGs also
preserve the clipped soft-edge filter consistently across Qt image backends.
Regenerate with `rsvg-convert -w 3060 logo-transparent-dark.svg -o logo-transparent-dark.png`
and the corresponding light variant. Apply overall UI opacity at display time.
Wordmark-only masters and their earlier PNG derivatives remain available for
uses without a subtitle.

Letter outlines derive from JetBrainsMono Nerd Font Regular; its SIL Open Font
License is included in `OFL-JetBrainsMono.txt`. Original artwork follows the
repository's GPL-3.0-or-later licence.

The separately selected primary application icon is study 09, Woven b; study 12,
Colourlight, is the alternate. This wordmark does not replace either icon.

## ASCII animations

The About dialog plays one of five ASCII animations, picked at random each time
it opens: a chromatogram trace, a cladogram with species brackets, an alignment,
a sequence loom and a paper shuttle. The cladogram is drawn wide and shown at
80% of the font size, filling the dialog width without being taller than the others. On dark themes they use the
dark wordmark's background, letter ink, sage and amber, plus slate blue and
terracotta for C and T; light themes get darker tones of the same roles.

`ascii/generate.py` draws them. `python3 ascii/generate.py export` regenerates
`../qml/AsciiArt.js`, which stores each distinct frame once; edit the script,
not the generated file. `python3 ascii/generate.py play <name>` plays one in a
terminal.
