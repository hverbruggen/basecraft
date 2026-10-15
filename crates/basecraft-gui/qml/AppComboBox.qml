import QtQuick
import QtQuick.Controls.Basic as Controls

Controls.ComboBox {
    id: combo
    leftPadding: 10
    rightPadding: 30
    topPadding: 6
    bottomPadding: 6
    background: AppControlBackground { control: combo }
    contentItem: Text {
        text: combo.displayText
        font: combo.font
        color: combo.palette.buttonText
        opacity: combo.enabled ? 1 : 0.45
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
    indicator: AppIcon {
        x: combo.width - width - 8
        width: 16
        height: combo.height
        name: "down"
        size: 12
        color: combo.palette.buttonText
    }
    delegate: Controls.ItemDelegate {
        required property int index
        width: combo.width
        text: combo.textAt(index)
        highlighted: combo.highlightedIndex === index
        contentItem: Text {
            text: parent.text
            font: combo.font
            color: parent.highlighted ? combo.palette.highlightedText : combo.palette.text
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
        background: Rectangle { color: parent.highlighted ? combo.palette.highlight : combo.palette.base }
    }
    popup: Controls.Popup {
        y: combo.height + 2
        width: combo.width
        padding: 1
        implicitHeight: Math.min(contentItem.implicitHeight + 2, 300)
        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: combo.popup.visible ? combo.delegateModel : null
            currentIndex: combo.highlightedIndex
            Controls.ScrollIndicator.vertical: Controls.ScrollIndicator {}
        }
        background: Rectangle { color: combo.palette.base; border.color: Qt.alpha(combo.palette.text, 0.28); radius: 3 }
    }
}
