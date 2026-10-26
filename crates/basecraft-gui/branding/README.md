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

The matching 3060-pixel PNGs are runtime derivatives of the SVG masters. They
preserve the clipped soft-edge filter consistently across Qt image backends.
Regenerate with `rsvg-convert -w 3060 wordmark-dark.svg -o wordmark-dark.png`
and the corresponding light variant. Apply overall UI opacity at display time.

Letter outlines derive from JetBrainsMono Nerd Font Regular; its SIL Open Font
License is included in `OFL-JetBrainsMono.txt`. Original artwork follows the
repository's GPL-3.0-or-later licence.

The separately selected primary application icon is study 09, Woven b; study 12,
Colourlight, is the alternate. This wordmark does not replace either icon.
