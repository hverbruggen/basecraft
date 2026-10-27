# Figure rasterization

SVG and PNG use the same SVG string from each controller's existing layout.
SVG output remains the original editable text/vector document. The private
usvg tree is used only to rasterize PNG; it is never serialized as an export.
No external renderer, temporary SVG or separate layout is involved.

The existing figure options dialog offers PNG width in pixels, initially 1500.
The rasterizer accepts finite whole widths from 1 to 10,000, rounds proportional
height to the nearest pixel, and rejects heights outside 1–10,000 or more than
40 million output pixels before allocating the output image. PNG encoding
finishes before opening the existing atomic export writer. Controller, source
filename, region, colours and width stay attached to the pending export when
another document becomes current.

resvg 0.45.1 is pinned for the workspace's Rust 1.85 minimum (resvg MSRV 1.67.1;
0.46+ requires 1.87). Only text and system/mapped-font support are enabled;
figures do not embed bitmap images. Fontconfig/system fonts resolve explicit
families and generic fallbacks; unavailable Unicode glyphs produce an error
instead of a PNG with missing text. Font discovery is cached until restart.
`licenses/png-renderer.txt` retains notices for the added locked dependencies
and is included in the packaged GUI bundle's `share/licenses/basecraft-gui`.

This is a presentation export change to the existing five GUI controllers;
there is no new scientific operation or persistence type. CLI/Python/MCP
interfaces are unchanged. It does not establish the cause of warnings from
other QML/native font paths.
