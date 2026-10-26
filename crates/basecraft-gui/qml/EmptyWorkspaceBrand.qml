import QtQuick

// Render the approved composition as one image so the subtitle cannot reflow.
Item {
    id: root
    property color backgroundColor: "white"
    readonly property bool darkBackground: 0.2126 * backgroundColor.r
        + 0.7152 * backgroundColor.g + 0.0722 * backgroundColor.b < 0.5
    opacity: 0.7

    Image {
        objectName: "emptyWorkspaceLogo"
        anchors.centerIn: parent
        // Allocate one quarter of the panel area, then fit unusually narrow panes.
        width: Math.min(Math.sqrt(parent.width * parent.height * 0.25 * 1020 / 480),
                        parent.width * 0.9, parent.height * 0.9 * 1020 / 480)
        height: width * 480 / 1020
        source: Qt.resolvedUrl("../branding/logo-transparent-" + (root.darkBackground ? "dark" : "light") + ".png")
        fillMode: Image.PreserveAspectFit
        smooth: true
        mipmap: true
        Accessible.role: Accessible.Graphic
        Accessible.name: "Basecraft. A workbench for evolutionary biology and taxonomy, bringing together hands-on analysis, automation and AI assistance, with provenance built in."
    }
}
