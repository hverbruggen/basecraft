import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// The one review screen for every metadata import: what will change, and
// what was found (duplicates, conflicts, unmatched rows, bad values).
// Nothing changes until Apply.
AppContentDialog {
    id: dialog
    objectName: "metadataReview"
    title: "Review import"
    standardButtons: Dialog.Apply | Dialog.Cancel
    // MetadataController.plan, parsed.
    property var plan: ({ summary: "", issues: [], empty: true })
    // "pattern" or "mapping": what Apply can remember.
    property string remembers: "pattern"
    signal applyRequested(string remember)
    signal dropped()
    initialFocusItem: rememberName

    readonly property var kindNames: ({
        duplicate: "Duplicate", conflict: "Conflict", unmatched: "Unmatched",
        invalid: "Not imported", imprecise_date: "Date precision", ignored: "Ignored column"
    })
    function show(value, what) {
        plan = value
        remembers = what
        rememberName.text = ""
        open()
    }
    onOpened: standardButton(Dialog.Apply).enabled = Qt.binding(() => !dialog.plan.empty || rememberName.text.trim().length > 0)
    onApplied: { applyRequested(rememberName.text.trim()); close() }
    onRejected: dropped()

    body: ColumnLayout {
        spacing: 8
        Label {
            objectName: "reviewSummary"
            text: dialog.plan.empty ? "The import changes nothing." : "Will add or change: " + dialog.plan.summary + "."
            font.bold: true
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        Label {
            text: (dialog.plan.rows || 0) + " rows read, " + (dialog.plan.matched || 0) + " matched existing specimens"
                + ((dialog.plan.new_specimens || []).length ? ", new: " + dialog.plan.new_specimens.slice(0, 12).join(", ") + (dialog.plan.new_specimens.length > 12 ? " and more" : "") : "")
                + ((dialog.plan.columns || []).length ? ". New columns: " + dialog.plan.columns.join(", ") : "") + "."
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        Label {
            visible: (dialog.plan.issues || []).length > 0
            text: dialog.plan.issues.length + " things to check:"
            Layout.fillWidth: true
        }
        Repeater {
            model: dialog.plan.issues || []
            delegate: Label {
                required property var modelData
                text: (modelData.row ? "Row " + modelData.row + " · " : "") + (dialog.kindNames[modelData.kind] || modelData.kind)
                    + (modelData.included ? " (included)" : " (left out)") + ": " + modelData.message
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
        RowLayout {
            spacing: 6
            Label { id: rememberLabel; text: dialog.remembers === "pattern" ? "Save the header pattern as" : "Save the column mapping as" }
            TextField {
                id: rememberName
                objectName: "rememberName"
                placeholderText: "Name (optional)"
                Accessible.labelledBy: rememberLabel
                Accessible.name: rememberLabel.text
                Layout.preferredWidth: 200
            }
        }
    }
}
