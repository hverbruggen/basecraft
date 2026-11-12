import QtQuick

// The interface font: the desktop's monospace font, or a chosen family.
AppComboBox {
    required property var settings
    property var family: null
    readonly property var families: Qt.fontFamilies()
    Accessible.name: "Font"
    model: ["Desktop font"].concat(families)
    currentIndex: family ? Math.max(0, families.indexOf(family) + 1) : 0
    onActivated: index => settings.update("fontFamily", JSON.stringify(index === 0 ? null : families[index - 1]))
}
