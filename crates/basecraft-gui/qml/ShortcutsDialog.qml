import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Keyboard shortcuts, grouped like the ribbon. `show(name)` opens it with
// only that ribbon tab's shortcuts; `show("")` lists them all.
AppDialog {
    id: dialog
    title: first.length && first !== "Help" ? first + " shortcuts" : "Keyboard shortcuts"
    modal: true
    anchors.centerIn: Overlay.overlay
    implicitWidth: 560
    implicitHeight: 620
    standardButtons: Dialog.Close

    property string first: ""
    property var commands: []

    // Keyboard only, grouped by ribbon tab; "Editing" has no tab of its
    // own and shows in the full list.
    readonly property var groups: [
        { name: "Find", keys: [
            ["Enter / Shift+Enter", "Next / previous match while typing in Find"],
            ["Escape", "Close Find and return to the document"],
            ["Clear the query", "Remove Find highlights without changing selection"],
        ] },
        { name: "File", keys: [
            ["Ctrl+W", "Close the tab"],
            ["Ctrl+Q", "Quit, asking about unsaved changes"],
            ["Ctrl+Z", "Undo"],
            ["Ctrl+Shift+Z, Ctrl+Y", "Redo"],
        ] },
        { name: "View", keys: [
            ["Ctrl++  /  Ctrl+−", "Zoom in / out"],
            ["Ctrl+0", "Fit the whole sequence"],
            ["Page Up / Page Down", "Scroll a screen"],
        ] },
        { name: "Sequence", keys: [
            ["Enter", "Edit the selected annotation"],
            ["Delete", "Delete the selected annotation"],
            ["Ctrl+A", "Select the whole sequence (sequence view focus)"],
            ["Ctrl+C", "Copy the selected bases"],
            ["Ctrl+E", "Extract the selection as a new sequence"],
        ] },
        { name: "Alignment", keys: [
            ["Shift+wheel / Ctrl+wheel", "Scroll horizontally / zoom"],
            ["Insert", "Switch between inserting and typing over bases"],
            ["Letters, −", "Insert one per selected column, or type over the selection; − is a gap"],
            ["Space", "Insert a gap before the selection"],
            ["Backspace", "Remove the gaps left of the selection; in insert mode, bases too"],
            ["Delete", "Delete the selected bases; the rows close up"],
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
        { name: "Tree", keys: [
            ["Click a branch", "Select the clade below it"],
            ["← →", "Select the parent / first child"],
            ["↑ ↓", "Select the sibling above / below"],
            ["Enter, double-click", "Annotate the selected clade"],
            ["F2", "Rename the selected tip"],
            ["Ctrl++  /  Ctrl+−", "Zoom in / out, both ways"],
            ["Alt++  /  Alt+−", "Rows further apart / closer"],
            ["Ctrl+wheel", "Zoom both ways"],
            ["Alt+wheel", "Rows closer or further apart"],
            ["Ctrl+Shift+wheel", "Tree wider or narrower"],
            ["Escape", "Clear the selection"],
        ] },
        { name: "Settings", keys: [
            ["Ctrl+B", "Show or hide the files sidebar"],
        ] },
        { name: "Trace", keys: [
            ["DNA letters / Delete", "Edit the consensus base / mark a gap"],
            ["← →", "Move one consensus base"],
            ["Alt+← →, [ ]", "Previous / next unresolved issue"],
            ["Alt+Enter", "Accept this base and go to the next issue"],
            ["Ctrl+Z / Ctrl+Shift+Z", "Undo / redo consensus edits"],
            ["Ctrl+S", "Save review decisions"],
            ["F2", "Edit the consensus sequence ID"],
            ["Ctrl+Shift+A", "Add consensus to the source alignment"],
            ["Ctrl+Shift+E", "Export consensus to FASTA"],
            ["Ctrl+Shift+C", "Copy consensus as FASTA"],
            ["Ctrl+wheel", "Zoom the shared base axis"],
            ["Shift+wheel", "Scroll horizontally through consensus and traces"],
            ["Wheel", "Scroll vertically through taller traces"],
            ["Right-click a consensus position", "Set either end of the kept range here"],
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
        const shared = commands.filter(command => command.shortcut.toString().length)
            .map(command => [command.shortcut.toString(), command.text])
        const access = commands.filter(c => c.keyHint && (!first.length || c.hintScope === first)).map(c => [c.keyHint, c.text])
        return [{name: "Ribbon navigation", keys: [["Alt (release alone), F10", "Show access keys; Escape returns one level"], ["F6", "Move between regions; ribbon starts on its tab bar"], ["Tab / Shift+Tab", "Next / previous ribbon group"], ["Arrows, Enter / Space", "Browse commands (including unavailable explanations), then invoke"], ["Escape", "Return to the document"]]}, { name: "Application commands", keys: shared }, {name:"Access keys", keys:access}].concat(g.length ? g : groups)
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
            spacing: dialog.font.pixelSize * 1.2
            Repeater {
                model: dialog.opened ? dialog.shown() : []
                ColumnLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 4
                    Label {
                        text: modelData.name
                        font.bold: true
                        font.pixelSize: dialog.font.pixelSize * 1.12
                        color: palette.windowText
                    }
                    GridLayout {
                        Layout.fillWidth: true
                        Layout.leftMargin: dialog.font.pixelSize
                        columns: 2
                        columnSpacing: dialog.font.pixelSize * 1.3
                        rowSpacing: dialog.font.pixelSize * 0.3
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
