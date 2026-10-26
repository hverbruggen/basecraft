# Basecraft wordmark

Accepted full logo: the soft-gradient lowercase wordmark, made fully opaque
for reuse in study 20 (2026-10-26).
The folded b is emphasised; the compressed JetBrainsMono lettering has an opaque
face and gentle edge shading. Only the opening-window presentation applies
70% overall opacity.

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
