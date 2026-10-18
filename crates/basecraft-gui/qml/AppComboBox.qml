import QtQuick
import QtQuick.Layouts
import QtQuick.Controls.Basic as Controls

Controls.ComboBox {
    id: combo
    leftPadding: 10
    rightPadding: 30
    topPadding: 6
    bottomPadding: 6
    // As wide as the longest choice, so picking another one never resizes
    // the box or the dialog around it.
    FontMetrics { id: textMetrics; font: combo.font }
    // The choices are read directly, not through textAt(), so the compiled
    // binding depends on them; a character of slack covers measuring
    // differences at small sizes.
    readonly property real widestText: {
        const choices = Array.isArray(combo.model) ? combo.model : []
        let widest = 0
        for (const choice of choices) widest = Math.max(widest, textMetrics.advanceWidth(String(choice)))
        for (let i = choices.length; i < combo.count; i++) widest = Math.max(widest, textMetrics.advanceWidth(combo.textAt(i)))
        return Math.ceil(widest + textMetrics.averageCharacterWidth)
    }
    implicitWidth: Math.max(implicitBackgroundWidth + leftInset + rightInset, widestText + leftPadding + rightPadding)
    // No wider than its longest choice, but free to shrink in a narrow
    // window, eliding the current choice.
    Layout.fillWidth: true
    Layout.maximumWidth: implicitWidth
    Layout.minimumWidth: 0
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
