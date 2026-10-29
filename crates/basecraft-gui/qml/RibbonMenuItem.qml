import QtQuick
import QtQuick.Controls.Basic

MenuItem {
    property real sizeFactor: 1
    icon.width: 16 * sizeFactor
    icon.height: 16 * sizeFactor
    implicitWidth: implicitContentWidth + leftPadding + rightPadding
        + (indicator && indicator.visible ? indicator.width + spacing : 0)
}
