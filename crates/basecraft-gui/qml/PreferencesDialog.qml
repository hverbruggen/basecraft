import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

AppFormDialog {
    id: dialog
    title: "Preferences"
    standardButtons: Dialog.Close
    required property var settings
    required property var commands
    property int defaultFontSizePoints: 9
    property var values: settings.snapshot.length ? JSON.parse(settings.snapshot) : ({})
    readonly property var missingIds: values.customization
        ? Object.keys(values.customization.shortcuts).concat(values.customization.ribbon_hidden)
            .filter((id, index, ids) => ids.indexOf(id) === index && !commands.some(command => command.commandId === id)) : []
    function change(key, value) { settings.update(key, JSON.stringify(value)) }

    Label { text: "Appearance"; font.bold: true }
    Label { text: "Colours and the monospace font family follow the system theme. Font-size changes apply immediately."; wrapMode: Text.Wrap; Layout.fillWidth: true }
    SpinBox {
        objectName: "interfaceFontSize"
        Kirigami.FormData.label: "Interface font size (pt):"
        Accessible.name: "Interface font size in points"
        from: 6; to: 32; editable: true
        value: dialog.values.ui && dialog.values.ui.font_size_points !== null && dialog.values.ui.font_size_points !== undefined
            ? dialog.values.ui.font_size_points : dialog.defaultFontSizePoints
        onValueModified: dialog.change("fontSizePoints", value)
    }
    AppButton { objectName: "resetInterfaceFontSize"; text: "Use default size (" + dialog.defaultFontSizePoints + " pt)"; onClicked: dialog.change("fontSizePoints", null) }
    CheckBox { text: "Show GC content"; checked: dialog.values.ui ? dialog.values.ui.show_gc : true; onToggled: dialog.change("showGc", checked) }
    CheckBox { text: "Open files sidebar"; checked: dialog.values.ui ? dialog.values.ui.sidebar_open : true; onToggled: dialog.change("sidebarOpen", checked) }
    SpinBox { Kirigami.FormData.label: "Minimum ORF (codons):"; from: 1; to: 1000000; editable: true; value: dialog.values.ui ? dialog.values.ui.orf_min_codons : 100; onValueModified: dialog.change("orfMinCodons", value) }
    CheckBox { text: "Insert alignment bases (rather than overwrite)"; checked: dialog.values.ui ? dialog.values.ui.alignment_insert : true; onToggled: dialog.change("alignmentInsert", checked) }
    Label { text: "Files, provenance and recovery"; font.bold: true }
    Label { text: "These policy defaults are reserved for recovery and provenance workflows. They do not create history for existing files."; wrapMode: Text.Wrap; Layout.fillWidth: true }
    AppComboBox {
        Kirigami.FormData.label: "Provenance policy:"
        model: ["Portable companion", "Local only", "None"]
        property var policies: ["portable", "local-only", "none"]
        currentIndex: dialog.values.files ? policies.indexOf(dialog.values.files.provenance) : 0
        onActivated: dialog.change("provenance", policies[currentIndex])
    }
    CheckBox { text: "Enable unsaved-work recovery"; checked: dialog.values.files ? dialog.values.files.recovery : true; onToggled: dialog.change("recovery", checked) }
    Label { text: "Compute"; font.bold: true }
    SpinBox { Kirigami.FormData.label: "CPU threads per task:"; from: 1; to: 65536; editable: true; value: dialog.values.compute ? dialog.values.compute.local_cpu_threads : 4; onValueModified: dialog.change("localCpuThreads", value) }
    Label { text: "New tasks use this allocation, capped by available CPUs. Running tasks retain their allocation."; wrapMode: Text.Wrap; Layout.fillWidth: true }
    Label { text: "Ribbon and shortcuts"; font.bold: true }
    Label { text: "Use Ctrl, Alt, Shift or Meta with one key, or a function key. Canvas keys and F6 are reserved. Empty shortcuts disable the command's shortcut. Reset restores defaults. Unavailable command IDs remain stored for future availability."; wrapMode: Text.Wrap; Layout.fillWidth: true }
    ColumnLayout {
        Layout.fillWidth: true
        spacing: 4
    Repeater {
        model: dialog.opened ? dialog.commands : []
        delegate: RowLayout {
            id: row
            required property var modelData
            Layout.fillWidth: true
            Label { text: row.modelData.text; Layout.preferredWidth: 180; wrapMode: Text.Wrap }
            TextField {
                Layout.fillWidth: true
                Accessible.name: row.modelData.text + " shortcut"
                text: row.modelData.shortcut.toString()
                placeholderText: "No shortcut"
                maximumLength: 128
                onEditingFinished: dialog.change("shortcut", { id: row.modelData.commandId, value: text })
            }
            CheckBox {
                text: "Show"
                checked: row.modelData.visible
                onToggled: dialog.change("ribbonCommandHidden", { id: row.modelData.commandId, hidden: !checked })
            }
        }
    }
    }
    Label { visible: dialog.missingIds.length > 0; text: "Unavailable commands (retained): " + dialog.missingIds.join(", "); wrapMode: Text.Wrap; Layout.fillWidth: true }
    AppButton { text: "Reset ribbon and shortcuts"; onClicked: dialog.change("resetCustomization", null) }
    Label { text: "Plugins"; font.bold: true }
    Label { text: "Plugins use the shared compute allocation. Plugin installation and tool locations are managed separately."; wrapMode: Text.Wrap; Layout.fillWidth: true }
    AppButton { text: "Reload preferences from disk"; onClicked: dialog.settings.reload() }
}
