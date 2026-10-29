import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Bounded and virtualized: only visible specimen-link editors are instantiated.
// FocusScrollView keeps keyboard-selected editors inside the same viewport.
FocusScrollView {
    id: control
    property var rows: [] // [{id, label}]
    property var specimenIds: []
    property var links: ({})
    property string emptyText: "Unlinked"
    property alias view: list
    signal edited(string record, string specimen)
    FontMetrics { id: metrics; font: control.font }
    implicitWidth: metrics.averageCharacterWidth * 64
    implicitHeight: 200
    Layout.fillWidth: true
    Layout.fillHeight: true
    Layout.minimumHeight: metrics.height * 3
    Layout.maximumHeight: 240
    clip: true
    contentWidth: availableWidth
    ListView {
        id: list
        objectName: "specimenLinkRows"
        width: control.availableWidth
        height: control.availableHeight
        clip: true
        model: control.rows
        spacing: 4
        keyNavigationEnabled: true
        delegate: RowLayout {
            required property var modelData
            width: ListView.view.width
            height: implicitHeight
            spacing: 8
            Label {
                text: modelData.label
                elide: Text.ElideRight
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                ToolTip.text: text
                ToolTip.visible: tipHover.hovered
                HoverHandler { id: tipHover }
            }
            ComboBox {
                objectName: "specimenLinkChoice"
                Layout.preferredWidth: metrics.averageCharacterWidth * 22
                Layout.maximumWidth: list.width * 0.55
                model: [control.emptyText].concat(control.specimenIds)
                currentIndex: Math.max(0, model.indexOf(control.links[modelData.id] || control.emptyText))
                Accessible.name: "Project specimen for " + modelData.id
                onActivated: control.edited(modelData.id, currentIndex === 0 ? "" : currentText)
            }
        }
    }
}
