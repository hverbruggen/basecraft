# Interface icons

These path-only SVGs are embedded by `../icons.qrc`. UI icons do not require an
installed icon font. Text, including mathematical symbols and shortcut labels,
continues to follow the user's font. Qt's standard control indicators remain
provided by its control style.

The private-use glyphs were extracted from JetBrainsMono Nerd Font Regular,
Nerd Fonts 3.5.1 (Arch package `ttf-jetbrains-mono-nerd-basic`), and originate in
Microsoft's Codicons. Attribution: Microsoft, https://github.com/microsoft/vscode-codicons.
See LICENSE-CODICONS (CC BY 4.0). Modifications: extracted outlines, inverted the
font's Y axis, centred and scaled into a 16×16 viewBox with a one-unit margin.
The ordinary Unicode outlines come from JetBrains Mono; see LICENSE-JETBRAINS.
`history.svg`, `edit.svg`, `waveform.svg`, `provenance.svg`, `text-representation.svg`, and the
three `annotation-*.svg` icons and `delimitation.svg`, `tree.svg`, `matrix.svg`, `align.svg`, and `trim.svg` are original Basecraft artwork under the repository
licence.

To regenerate the extracted assets, install FontTools in a temporary environment
and run:

```sh
python crates/basecraft-gui/tools/export_icons.py /path/to/JetBrainsMonoNerdFont-Regular.ttf
```

FontTools is a development utility only, not a build or runtime dependency.
When adding an icon, also add it to `icons.qrc`. Reference icons by their semantic
filename through AppIcon or AppIconButton. Colours come from the control palette;
do not bake a theme colour into an SVG.
