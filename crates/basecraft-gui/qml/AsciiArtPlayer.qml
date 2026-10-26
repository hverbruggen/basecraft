import QtQuick
import "AsciiArt.js" as AsciiArt

// One of the generated ASCII animations, picked at random, drawn straight onto
// the surrounding background in the palette made for it. Decorative; plays only while visible.
Item {
    id: root
    property font font
    property bool dark: true
    readonly property var artPalette: AsciiArt.palettes[dark ? "dark" : "light"]
    property int animationIndex: Math.floor(Math.random() * AsciiArt.animations.length)
    property int frame: 0
    readonly property int animationCount: AsciiArt.animations.length
    readonly property var animation: AsciiArt.animations[animationIndex]
    // Some animations are drawn wider and shown smaller, to keep them low.
    readonly property font artFont: Qt.font(font.pointSize > 0
        ? { family: font.family, pointSize: font.pointSize * animation.scale }
        : { family: font.family, pixelSize: Math.max(1, Math.round(font.pixelSize * animation.scale)) })
    readonly property real cellWidth: cell.advanceWidth
    readonly property real lineHeight: metrics.height
    readonly property var current: {
        const cells = animation.unique[animation.frames[frame % animation.frames.length]]
        return { chars: cells[0].split("\n"), styles: cells[1].split("\n") }
    }

    implicitWidth: animation.columns * cellWidth
    implicitHeight: animation.rows * lineHeight
    Accessible.ignored: true
    onAnimationIndexChanged: frame = 0

    FontMetrics { id: metrics; font: root.artFont }
    // A property, unlike FontMetrics.advanceWidth(), so the width follows font changes.
    TextMetrics { id: cell; font: root.artFont; text: "M" }

    function rowText(row) {
        // The row repeater can briefly outlive a switch to a shorter animation.
        if (row >= current.chars.length) return ""
        const chars = current.chars[row], styles = current.styles[row]
        let out = "", run = "", style = null
        const flush = () => {
            if (!run.length) return
            const inverted = style in AsciiArt.inverse
            out += "<font color=\"" + (inverted ? artPalette.background : artPalette.colours[style]) + "\">" + run + "</font>"
            run = ""
        }
        for (let i = 0; i < chars.length; ++i) {
            if (styles[i] !== style) { flush(); style = styles[i] }
            const ch = chars[i]
            run += ch === " " ? "&nbsp;" : ch === "<" ? "&lt;" : ch === ">" ? "&gt;" : ch === "&" ? "&amp;" : ch
        }
        flush()
        return out
    }

    // StyledText has no background colour, so inverted cells get a rectangle underneath.
    readonly property var invertedCells: {
        const cells = []
        for (let row = 0; row < current.styles.length; ++row) {
            const styles = current.styles[row]
            for (let col = 0; col < styles.length; ++col)
                if (styles[col] in AsciiArt.inverse)
                    cells.push({ row: row, col: col, colour: artPalette.colours[AsciiArt.inverse[styles[col]]] })
        }
        return cells
    }

    Item {
        id: art
        objectName: "asciiArt"
        anchors.fill: parent

        Repeater {
            model: root.invertedCells
            Rectangle {
                x: modelData.col * root.cellWidth
                y: modelData.row * root.lineHeight
                width: root.cellWidth
                height: root.lineHeight
                color: modelData.colour
            }
        }
        Repeater {
            model: root.animation.rows
            Text {
                y: index * root.lineHeight
                height: root.lineHeight
                font: root.artFont
                textFormat: Text.StyledText
                text: root.rowText(index)
            }
        }
    }

    Timer {
        interval: 1000 / root.animation.fps
        repeat: true
        running: root.visible
        onTriggered: root.frame = (root.frame + 1) % root.animation.frames.length
    }
}
