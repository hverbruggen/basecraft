import QtQuick
import QtQuick.Controls.impl as ControlsImpl

// Isolated compatibility adapter: Kirigami.Icon mask tinting does not work on
// Qt's software scene graph. The pixel test covers this until a public renderer
// supports both backends.
Item {
    id: root
    property string name
    property color color: "black"
    property real size: 16
    implicitWidth: size
    implicitHeight: size
    ControlsImpl.IconImage {
        anchors.centerIn: parent
        width: root.size
        height: root.size
        source: root.name.length ? Qt.resolvedUrl("../icons/" + root.name + ".svg") : ""
        sourceSize: Qt.size(root.size, root.size)
        color: root.color
    }
}
