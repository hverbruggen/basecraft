import QtQuick
import QtQuick.Controls.Basic as Controls

Controls.Button {
    id: button
    leftPadding: 10
    rightPadding: 10
    topPadding: 6
    bottomPadding: 6
    background: AppControlBackground { control: button }
    contentItem: Text {
        text: button.text
        font: button.font
        color: button.palette.buttonText
        opacity: button.enabled ? 1 : 0.45
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
}
