import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Colours for the species of one hypothesis column. Applying an empty value
// returns that species to the theme colour.
AppDialog {
    id: dialog
    title: "Species colours" + (hypothesisName.length ? ": " + hypothesisName : "")
    standardButtons: Dialog.Ok | Dialog.Cancel
    property string hypothesisName: ""
    // [{id, label, members, color, theme}] in column order: `color` is drawn
    // now, `theme` is the colour without a custom choice.
    property var groups: []
    // Group ID to "#rrggbb", or "" for the theme colour.
    property var edits: ({})
    property int current: 0
    signal colorsChosen(var colors)
    function show(name, items) {
        hypothesisName = name
        groups = items
        edits = ({})
        current = 0
        hexField.text = colorOf(0)
        open()
    }
    function colorOf(index) {
        const g = groups[index]
        if (!g) return "#808080"
        if (!(g.id in edits)) return g.color
        return edits[g.id].length ? edits[g.id] : g.theme
    }
    function setColor(index, value) {
        const next = Object.assign({}, edits)
        next[groups[index].id] = value
        edits = next
    }
    readonly property bool validHex: /^#[0-9a-fA-F]{6}$/.test(hexField.text)
    onAccepted: colorsChosen(edits)
    onOpened: standardButton(Dialog.Ok).enabled = Qt.binding(() => dialog.validHex || dialog.groups.length === 0)
    initialFocusItem: list

    contentItem: RowLayout {
        spacing: 16
        ListView {
            id: list
            objectName: "speciesColorList"
            Layout.preferredWidth: 220
            Layout.fillHeight: true
            Layout.minimumHeight: 240
            clip: true
            model: dialog.groups
            currentIndex: dialog.current
            onCurrentIndexChanged: dialog.current = currentIndex
            keyNavigationEnabled: true
            Accessible.name: "Species"
            delegate: ItemDelegate {
                required property var modelData
                required property int index
                width: ListView.view.width
                highlighted: ListView.isCurrentItem
                onClicked: dialog.current = index
                contentItem: RowLayout {
                    Rectangle {
                        width: 16; height: 16; radius: 3
                        color: dialog.colorOf(index)
                        border.color: dialog.palette.mid
                    }
                    Label {
                        text: modelData.label + " (" + modelData.members + ")"
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }
                Accessible.name: modelData.label + ", " + dialog.colorOf(index)
            }
        }
        ColumnLayout {
            ColorWheel {
                id: wheel
                objectName: "colorWheel"
                color: dialog.colorOf(dialog.current)
                onEdited: value => {
                    dialog.setColor(dialog.current, value.toString())
                    hexField.text = value.toString()
                }
            }
            RowLayout {
                Label { id: hexLabel; text: "Hex" }
                TextField {
                    id: hexField
                    objectName: "colorHex"
                    Layout.preferredWidth: 100
                    maximumLength: 7
                    font.family: "monospace"
                    Accessible.name: hexLabel.text
                    onTextEdited: if (/^#[0-9a-fA-F]{6}$/.test(text)) dialog.setColor(dialog.current, text.toLowerCase())
                }
                Rectangle {
                    width: 28; height: 20; radius: 3
                    color: dialog.colorOf(dialog.current)
                    border.color: dialog.palette.mid
                }
            }
            Label {
                visible: !dialog.validHex
                text: "Use #rrggbb"
                color: dialog.palette.highlight
            }
            AppButton {
                text: "Use theme colours"
                onClicked: {
                    const next = {}
                    for (const g of dialog.groups) next[g.id] = ""
                    dialog.edits = next
                    // Show the theme colour that applies after OK.
                    hexField.text = dialog.colorOf(dialog.current)
                }
            }
        }
    }
    onCurrentChanged: hexField.text = colorOf(current)
}
