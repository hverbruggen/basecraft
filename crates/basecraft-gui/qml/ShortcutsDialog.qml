import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Keyboard shortcuts, grouped like the ribbon. `show(name)` opens it with
// only that ribbon tab's shortcuts; `show("")` lists them all.
Dialog {
    id: dialog
    title: first.length && first !== "Help" ? first + " shortcuts" : "Keyboard shortcuts"
    modal: true
    anchors.centerIn: Overlay.overlay
    width: Math.min(560, parent ? parent.width - 40 : 560)
    height: Math.min(620, parent ? parent.height - 40 : 620)
    standardButtons: Dialog.Close

    property string first: ""

    // Keyboard only, grouped by ribbon tab; "Editing" has no tab of its
    // own and shows in the full list.
    readonly property var groups: [
        { name: "File", keys: [
            ["Ctrl+S", "Save"],
            ["Ctrl+Z", "Undo"],
            ["Ctrl+Shift+Z, Ctrl+Y", "Redo"],
        ] },
        { name: "View", keys: [
            ["+  /  −", "Zoom in / out"],
            ["Ctrl+0", "Fit the whole sequence"],
            ["Page Up / Page Down", "Scroll a screen"],
        ] },
        { name: "Annotations", keys: [
            ["Ctrl+N", "Annotate the selection"],
            ["Ctrl+Shift+N", "Annotate the selection on the reverse strand"],
            ["Enter", "Edit the selected annotation"],
            ["Delete", "Delete the selected annotation"],
            ["Ctrl+C", "Copy the selected bases"],
            ["Ctrl+E", "Extract the selection as a new sequence"],
        ] },
        { name: "Alignment", keys: [
            ["Letters", "Type over the residue at the cursor"],
            ["Space", "Insert a gap before the selection"],
            ["Backspace", "Remove the gaps left of the selection"],
            ["Delete", "Delete the selected residues; the rows close up"],
            ["Alt+← →, or drag", "Slide the selection into gaps"],
            ["Alt+↑ ↓", "Move the selected rows"],
            ["F2", "Rename the cursor's row"],
            ["Arrows, Ctrl+arrows", "Move the cursor 1 or 10 cells"],
            ["Shift+arrows, Shift+click", "Extend the selection"],
            ["Home / End", "First / last column (Ctrl: first / last row too)"],
            ["Ctrl+A", "Select everything"],
            ["Ctrl+C", "Copy the selected block as FASTA"],
            ["Escape", "Select just the cursor's cell"],
        ] },
        { name: "Settings", keys: [
            ["Ctrl+B", "Show or hide the files sidebar"],
        ] },
        { name: "Editing", keys: [
            ["Letters", "Type bases at the cursor, replacing a selection"],
            ["Backspace / Delete", "Delete before / after the cursor, or the selection"],
            ["← →", "Move the cursor"],
            ["Ctrl+← →", "Move the cursor 10 bases"],
            ["Shift+arrows", "Extend the selection"],
            ["↑ ↓", "Move the cursor a line"],
            ["Home / End", "Start / end of the sequence"],
            ["Escape", "Clear the selection"],
        ] },
    ]

    // A tab's name shows only its group; "" (Help) shows them all.
    function shown() {
        const g = groups.filter(x => x.name === first)
        return g.length ? g : groups
    }

    function show(name) {
        first = name
        open()
    }

    ScrollView {
        anchors.fill: parent
        clip: true
        ColumnLayout {
            width: dialog.availableWidth
            spacing: 14
            Repeater {
                model: dialog.opened ? dialog.shown() : []
                ColumnLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 4
                    Label {
                        text: modelData.name
                        font.bold: true
                        color: palette.windowText
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
