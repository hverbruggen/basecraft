import QtQuick

// The Basecraft wordmark in the variant drawn for the given background.
// Decorative: the surrounding view supplies any accessible text.
Image {
    id: logo
    property color backgroundColor: "white"
    readonly property bool darkBackground: 0.2126 * backgroundColor.r
        + 0.7152 * backgroundColor.g + 0.0722 * backgroundColor.b < 0.5
    readonly property string tagline: "A workbench for evolutionary biology and taxonomy, bringing together hands-on analysis, automation and AI assistance, with provenance built in."
    readonly property color taglineColor: darkBackground ? "#81978f" : "#83958b"
    height: width * 420 / 1020
    source: Qt.resolvedUrl("../branding/wordmark-" + (darkBackground ? "dark" : "light") + ".png")
    fillMode: Image.PreserveAspectFit
    smooth: true
    mipmap: true
    Accessible.ignored: true
}
