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
    property var result: ({count: 0, current: 0, rows: []})
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
        result = ({count: 0, current: 0, rows: []})
        if (documentFocusItem) documentFocusItem.forceActiveFocus(Qt.ShortcutFocusReason)
    }
    function submit() {
        controller.find_query(field.text, reverse.checked)
        result = ({busy: field.text.length > 0, count: 0, current: 0, rows: []})
        debounce.restart()
    }
    function poll(direction) {
        if (debounce.running) return
        const previous = result
        const reply = controller.find_poll(direction)
        if (!reply.length) return
        result = JSON.parse(reply)
        if (result.count > 0 && (direction !== 0 || previous.busy || previous.current !== result.current))
            revealed(result)
    }
    onControllerChanged: {
        if (previousController && previousController !== controller) previousController.find_query("", false)
        previousController = controller
        result = ({count: 0, current: 0, rows: []})
        if (opened && controller) submit()
    }
    onOpenedChanged: if (opened) submit()
    Component.onDestruction: if (controller) controller.find_query("", false)
    Keys.onEscapePressed: event => { closeFind(); event.accepted = true }
    Timer { id: debounce; interval: 160; onTriggered: bar.poll(0) }
    Timer { interval: 80; repeat: true; running: bar.opened && bar.activeDocument && !debounce.running; onTriggered: bar.poll(0) }

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
                        if (debounce.running) { debounce.stop(); bar.poll(0) }
                        else bar.poll(event.modifiers & Qt.ShiftModifier ? -1 : 1)
                        event.accepted = true
                    }
                }
            }
            AppIconButton { iconName: "left"; description: "Previous match (Shift+Enter)"; Accessible.name: "Previous match"; enabled: bar.result.count > 0; onClicked: bar.poll(-1) }
            AppIconButton { iconName: "right"; description: "Next match (Enter)"; Accessible.name: "Next match"; enabled: bar.result.count > 0; onClicked: bar.poll(1) }
            AppIconButton { iconName: "close"; description: "Close Find (Escape)"; Accessible.name: "Close Find"; onClicked: bar.closeFind() }
        }
        RowLayout {
            Layout.fillWidth: true
            AppCheckBox {
                id: reverse
                visible: bar.sequenceSearch
                text: "Reverse complement"
                enabled: bar.result.nucleotide === true || checked
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
                    : bar.result.busy ? "Finding…"
                    : (bar.result.count ? bar.result.current + " of " + bar.result.count + (bar.result.limited ? " (limit reached; narrow query)" : "") + " · " + bar.result.label + (bar.result.start ? " · " + bar.result.start : "") + (bar.result.reverse ? " · reverse" : "") : "No matches")
                        + (bar.result.explanation ? " · " + bar.result.explanation : "")
                Accessible.name: text
            }
        }
    }
}
