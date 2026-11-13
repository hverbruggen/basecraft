import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// One transient Find session for its document. Refresh/navigation never takes
// typing focus. The caller owns only document focus and vertical row reveal.
Control {
    id: bar
    z: 10
    property var previousController: null
    required property var controller
    required property var documentFocusItem
    property bool sequenceSearch: true
    property bool nameSearch: true
    property string inputHint: "Name or sequence motif"
    property bool opened: false
    property bool activeDocument: true
    property alias query: field.text
    property alias queryField: field
    // The controller asks for a reveal by counting up `find_reveals`.
    readonly property int reveals: opened && activeDocument && controller ? controller.find_reveals : 0
    onRevealsChanged: if (reveals && !debounce.running) revealed({row: controller.find_row, y: controller.find_hit_y})
    signal revealed(var match)
    visible: opened
    height: opened ? implicitHeight : 0
    padding: metrics.height * 0.35
    implicitHeight: contentItem.implicitHeight + topPadding + bottomPadding
    FontMetrics { id: metrics; font: field.font }
    background: Rectangle { color: bar.palette.window; border.color: bar.palette.mid }

    function openFind() {
        opened = true
        field.forceActiveFocus(Qt.ShortcutFocusReason)
        field.selectAll()
    }
    function closeFind() {
        debounce.stop()
        controller.find_query("", false)
        opened = false
        if (documentFocusItem) documentFocusItem.forceActiveFocus(Qt.ShortcutFocusReason)
    }
    function submit() {
        controller.find_query(field.text, reverse.checked)
        debounce.restart()
    }
    onControllerChanged: {
        if (previousController && previousController !== controller) previousController.find_query("", false)
        previousController = controller
        if (opened && controller) submit()
    }
    onOpenedChanged: if (opened) submit()
    Component.onDestruction: if (controller) controller.find_query("", false)
    Keys.onEscapePressed: event => { closeFind(); event.accepted = true }
    Timer { id: debounce; interval: 160; onTriggered: bar.controller.find_step(0) }

    contentItem: ColumnLayout {
        spacing: metrics.height * 0.2
        RowLayout {
            Layout.fillWidth: true
            spacing: metrics.height * 0.3
            Label { id: caption; text: "Find" }
            TextField {
                id: field
                objectName: "findQuery"
                Layout.fillWidth: true
                Layout.minimumWidth: metrics.height * 3
                placeholderText: bar.inputHint
                ToolTip.visible: hovered
                ToolTip.text: "Names match literal text, ignoring case. Motifs ignore document gaps and include overlaps. Nucleotide IUPAC sets must intersect (T = U); protein symbols match literally. Reverse complement adds the other strand."
                Accessible.name: "Find in this document"
                Accessible.labelledBy: caption
                selectByMouse: true
                onTextEdited: bar.submit()
                Keys.onPressed: event => {
                    if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                        if (debounce.running) { debounce.stop(); bar.controller.find_step(0) }
                        else bar.controller.find_step(event.modifiers & Qt.ShiftModifier ? -1 : 1)
                        event.accepted = true
                    }
                }
            }
            AppIconButton { iconName: "left"; description: "Previous match (Shift+Enter)"; Accessible.name: "Previous match"; enabled: bar.controller.find_count > 0; onClicked: bar.controller.find_step(-1) }
            AppIconButton { iconName: "right"; description: "Next match (Enter)"; Accessible.name: "Next match"; enabled: bar.controller.find_count > 0; onClicked: bar.controller.find_step(1) }
            AppIconButton { iconName: "close"; description: "Close Find (Escape)"; Accessible.name: "Close Find"; onClicked: bar.closeFind() }
        }
        RowLayout {
            Layout.fillWidth: true
            AppCheckBox {
                id: reverse
                visible: bar.sequenceSearch
                text: "Reverse complement"
                enabled: bar.controller.find_nucleotide || checked
                Accessible.name: "Also find reverse complement"
                ToolTip.visible: hovered
                ToolTip.text: "Also search the reverse complement; available for nucleotide sequences"
                onClicked: bar.submit()
            }
            Label {
                objectName: "findStatus"
                Layout.fillWidth: true
                Layout.preferredHeight: metrics.height * 2
                Layout.minimumHeight: metrics.height * 2
                maximumLineCount: 2
                elide: Text.ElideRight
                ToolTip.visible: statusHover.hovered
                ToolTip.text: text
                HoverHandler { id: statusHover }
                wrapMode: Text.Wrap
                textFormat: Text.PlainText
                text: !field.text.length ? (bar.sequenceSearch ? (bar.nameSearch ? "Names or motifs · IUPAC nucleotides; literal protein symbols" : "Motifs: IUPAC nucleotides") : "Taxon names: literal text, ignoring case")
                    : bar.controller.find_status
                Accessible.name: text
            }
        }
    }
}
