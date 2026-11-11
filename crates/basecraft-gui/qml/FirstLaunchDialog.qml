import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Asked once, at first launch: how the window should look, and whether to
// hear about new releases. Every answer applies at once and can be changed
// later in Preferences.
AppFormDialog {
    id: dialog
    objectName: "firstLaunchDialog"
    title: "Welcome to Basecraft"
    standardButtons: Dialog.Ok
    required property var settings
    property int defaultFontSizePoints: 9
    property var values: settings.snapshot.length ? JSON.parse(settings.snapshot) : ({})
    readonly property var ui: values.ui || ({})
    readonly property var themes: ["follow-desktop", "light", "dark"]
    function change(key, value) { settings.update(key, JSON.stringify(value)) }
    initialFocusItem: fontSize
    onClosed: change("welcomed", true)

    Label { text: "Choose how Basecraft looks. You can change these later in Preferences."; wrapMode: Text.Wrap; Layout.fillWidth: true }
    AppSpinBox {
        id: fontSize
        objectName: "firstFontSize"
        Kirigami.FormData.label: "Interface font size (pt):"
        Accessible.name: "Interface font size in points"
        from: 6; to: 32; editable: true
        value: dialog.ui.font_size_points !== null && dialog.ui.font_size_points !== undefined ? dialog.ui.font_size_points : dialog.defaultFontSizePoints
        onValueModified: dialog.change("fontSizePoints", value)
    }
    AppSpinBox {
        objectName: "firstRibbonSize"
        Kirigami.FormData.label: "Ribbon size (%):"
        Accessible.name: "Ribbon size in percent"
        from: 100; to: 200; stepSize: 25; editable: true
        value: dialog.ui.ribbon_size_percent !== undefined ? dialog.ui.ribbon_size_percent : 100
        onValueModified: dialog.change("ribbonSizePercent", value)
    }
    AppComboBox {
        objectName: "firstTheme"
        Kirigami.FormData.label: "Theme:"
        Accessible.name: "Theme"
        model: ["Follow the desktop", "Light", "Dark"]
        currentIndex: Math.max(0, dialog.themes.indexOf(dialog.ui.theme || "follow-desktop"))
        onActivated: index => dialog.change("theme", dialog.themes[index])
    }
    AppCheckBox {
        objectName: "firstColourBlind"
        text: "Colour-blind colours for data"
        checked: dialog.ui.data_color_scheme === "color-blind-friendly"
        onToggled: dialog.change("colorBlindFriendly", checked)
    }
    AppCheckBox {
        objectName: "firstReleaseNotices"
        text: "Tell me when a new version is out"
        Accessible.description: "Checks the project's release page on GitHub at startup; nothing else is sent."
        checked: dialog.ui.release_notices === true
        onToggled: dialog.change("releaseNotices", checked)
    }
}
