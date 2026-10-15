"""Extract the UI outlines once; requires fonttools, not needed to build/run.

Usage: python export_icons.py /path/to/JetBrainsMonoNerdFont-Regular.ttf
"""
import sys
from pathlib import Path
from fontTools.ttLib import TTFont
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen

ICONS = {
    'keyboard': 0xea65, 'open': 0xeaf7, 'save': 0xeb4b, 'save-as': 0xeb4a,
    'project': 0xebdf, 'zoom-out': 0xeb82, 'zoom-in': 0xeb81,
    'fit': 0xeb4c, 'selection': 0xebf8, 'info': 0xea74,
    'link': 0xea84, 'bug': 0xeaaf, 'columns': 0xeabf,
    'delete': 0xea81, 'new-file': 0xea73, 'add-file': 0xea7b,
    'previous': 0xea9b, 'next': 0xea9c, 'accept': 0xeab2,
    'add': 0x2b,
    'up': 0x25b2, 'down': 0x25bc, 'left': 0x25c0, 'right': 0x25b6,
    'close': 0xd7, 'more': 0x22ef, 'minus': 0x2212,
    'arrow-left': 0x2190, 'arrow-right': 0x2192,
}

if __name__ == '__main__':
    font = TTFont(sys.argv[1])
    glyphs = font.getGlyphSet()
    cmap = font.getBestCmap()
    out = Path(__file__).resolve().parents[1] / 'icons'
    for name, code in ICONS.items():
        glyph = glyphs[cmap[code]]
        bounds = BoundsPen(glyphs)
        glyph.draw(bounds)
        x0, y0, x1, y1 = bounds.bounds
        scale = 14 / max(x1 - x0, y1 - y0)
        pen = SVGPathPen(glyphs, ntos=lambda n: f'{n:.4f}'.rstrip('0').rstrip('.') if n else '0')
        glyph.draw(TransformPen(pen, (scale, 0, 0, -scale,
                                     8 - scale * (x0 + x1) / 2,
                                     8 + scale * (y0 + y1) / 2)))
        (out / f'{name}.svg').write_text(
            '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16">'
            f'<path d="{pen.getCommands()}"/></svg>\n')
