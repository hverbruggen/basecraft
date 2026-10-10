import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Keyboard and pointer shortcuts, grouped like the ribbon. `show(name)`
// opens it with that ribbon tab's group first.
Dialog {
    id: dialog
    title: "Keyboard shortcuts"
    modal: true
    anchors.centerIn: Overlay.overlay
    width: Math.min(560, parent ? parent.width - 40 : 560)
    height: Math.min(620, parent ? parent.height - 40 : 620)
    standardButtons: Dialog.Close

    property string first: ""

    readonly property var groups: [
        { name: "File", keys: [
            ["Ctrl+S", "Save"],
            ["Ctrl+Z", "Undo"],
            ["Ctrl+Shift+Z, Ctrl+Y", "Redo"],
        ] },
        { name: "View", keys: [
            ["+  /  −", "Zoom in / out"],
            ["Ctrl+0", "Fit the whole sequence"],
            ["Double-click", "Zoom in there"],
            ["Wheel", "Scroll lines (wrapped) or zoom (linear)"],
            ["Ctrl+wheel, pinch", "Zoom"],
            ["Shift+wheel", "Scroll sideways (linear)"],
            ["Middle-drag", "Move the view"],
            ["Page Up / Page Down", "Scroll a screen (wrapped)"],
            ["F2", "Next layout: wrapped, linear, circular"],
            ["Ctrl+B", "Show or hide the files"],
        ] },
        { name: "Annotations", keys: [
            ["Click", "Select an annotation and its bases"],
            ["Double-click", "Edit an annotation"],
            ["Enter", "Edit the selected annotation"],
            ["Ctrl+N", "Annotate the selection (Shift: reverse strand)"],
            ["Delete", "Delete the selected annotation"],
            ["Ctrl+C", "Copy the selected bases"],
            ["Ctrl+E", "Extract the selection as a new sequence"],
            ["Right-click", "Actions for what's under the pointer"],
        ] },
        { name: "Bases", keys: [
            ["Click", "Place the cursor"],
            ["Drag", "Select bases (past an edge scrolls)"],
            ["Shift+click", "Extend the selection"],
            ["Letters", "Type bases at the cursor, replacing a selection"],
            ["Backspace / Delete", "Delete before / after the cursor, or the selection"],
            ["← →", "Move the cursor (Ctrl: 10 bases, Shift: extend)"],
            ["↑ ↓", "Move a line (wrapped)"],
            ["Home / End", "Start / end of the sequence"],
            ["Escape", "Clear the selection"],
        ] },
    ]

    function show(name) {
        first = name
        open()
    }

    function ordered() {
        const g = groups.slice()
        const i = g.findIndex(x => x.name === first)
        if (i > 0) g.unshift(g.splice(i, 1)[0])
        return g
    }

    ScrollView {
        anchors.fill: parent
        clip: true
        ColumnLayout {
            width: dialog.availableWidth
            spacing: 14
            Repeater {
                model: dialog.opened ? dialog.ordered() : []
                ColumnLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 4
                    Label {
                        text: modelData.name
                        font.bold: true
                        color: modelData.name === dialog.first ? palette.highlight : palette.windowText
                    }
                    GridLayout {
                        columns: 2
                        columnSpacing: 16
                        rowSpacing: 3
                        Repeater {
                            model: modelData.keys
                            delegate: Label {
                                required property var modelData
                                required property int index
                                // Keys and descriptions alternate across the two columns.
                                Layout.row: Math.floor(index)
                                Layout.column: 0
                                text: modelData[0]
                                font.bold: true
                                opacity: 0.85
                            }
                        }
                        Repeater {
                            model: modelData.keys
                            delegate: Label {
                                required property var modelData
                                required property int index
                                Layout.row: index
                                Layout.column: 1
                                Layout.fillWidth: true
                                text: modelData[1]
                                wrapMode: Text.Wrap
                            }
                        }
                    }
                }
            }
        }
    }
}
