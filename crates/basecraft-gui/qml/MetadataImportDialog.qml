import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Say what each FASTA header field or spreadsheet column holds. Planning
// changes nothing: the result opens in the review dialog, where it is
// applied or dropped.
AppContentDialog {
    id: dialog
    objectName: "metadataImport"
    standardButtons: Dialog.Cancel
    // "headers" or "sheet".
    property string mode: "headers"
    property string file: ""
    // Header mode: MetadataController.header_preview; sheet mode: sheet_preview.
    property var preview: ({})
    // Columns of the specimen table, then known Darwin Core terms.
    property var columns: []
    // Saved header patterns: [{name, separator, fields}].
    property var patterns: []
    property string separator: "|"
    // Field index (headers) or column header (sheet) to role name.
    property var roles: ({})
    property string message: ""
    // Header mode re-reads the preview when the separator changes.
    signal previewRequested(string separator)
    signal planRequested(string separator, string roles, bool createSpecimens, bool overwrite)
    title: mode === "headers" ? "Import from FASTA headers" : "Import from a spreadsheet"
    initialFocusItem: mode === "headers" ? separatorField : planButton

    readonly property var roleNames: {
        const base = [["ignore", "Not imported"], ["specimen", "Specimen ID"], ["taxon", "Taxon name"]]
        if (mode === "sheet") base.push(["record", "Sequence record ID"])
        for (const name of columns) base.push(["column:" + name, name])
        return base
    }
    readonly property var fieldKeys: mode === "headers"
        ? Array.from({ length: preview.fields || 0 }, (_, i) => String(i))
        : (preview.headers || []).filter(h => h.length > 0)
    readonly property bool hasSpecimen: Object.values(roles).includes("specimen")

    function show(kind, path, data, tableColumns, savedPatterns) {
        mode = kind
        file = path
        columns = tableColumns
        patterns = savedPatterns || []
        message = ""
        setPreview(data)
        if (kind === "headers") {
            const roles0 = {}
            for (let i = 0; i < (data.fields || 0); i++) roles0[String(i)] = i === 0 ? "specimen" : "ignore"
            roles = roles0
        } else {
            roles = Object.assign({}, data.mapping || {})
        }
        open()
    }
    function setPreview(data) {
        preview = data
        message = data.error || ""
    }
    function usePattern(pattern) {
        separator = pattern.separator
        separatorField.text = pattern.separator
        previewRequested(pattern.separator)
        const next = {}
        pattern.fields.forEach((f, i) => next[String(i)] = f.role === "column" ? "column:" + f.column : f.role)
        roles = next
    }
    function setRole(key, role) {
        const next = Object.assign({}, roles)
        next[key] = role
        roles = next
    }
    function roleList() {
        if (mode === "sheet") return JSON.stringify(roles)
        return JSON.stringify(fieldKeys.map(k => roles[k] || "ignore"))
    }

    body: ColumnLayout {
        spacing: 8
        Label {
            text: dialog.file
            elide: Text.ElideMiddle
            Layout.fillWidth: true
        }
        AppFormRow {
            visible: dialog.mode === "headers"
            RowLayout {
                spacing: 6
                Label { id: separatorLabel; text: "Field separator" }
                TextField {
                    id: separatorField
                    objectName: "headerSeparator"
                    text: dialog.separator
                    Accessible.labelledBy: separatorLabel
                    Accessible.name: separatorLabel.text
                    Layout.preferredWidth: 60
                    onTextEdited: { dialog.separator = text; dialog.previewRequested(text) }
                }
            }
            RowLayout {
                visible: dialog.patterns.length > 0
                spacing: 6
                Label { id: savedLabel; text: "Saved pattern" }
                AppComboBox {
                    id: savedPatterns
                    Accessible.name: savedLabel.text
                    model: dialog.patterns.map(p => p.name)
                    onActivated: index => dialog.usePattern(dialog.patterns[index])
                }
            }
        }
        Label {
            text: dialog.mode === "headers"
                ? (dialog.preview.records || 0) + " records. Each header line is split at the separator; say what each field holds."
                : (dialog.preview.count || 0) + " rows" + (dialog.preview.remembered ? ", using the saved mapping \"" + dialog.preview.remembered + "\"" : "") + ". Say what each column holds."
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        GridLayout {
            columns: 2
            columnSpacing: 12
            rowSpacing: 4
            Repeater {
                model: dialog.fieldKeys
                delegate: Label {
                    required property string modelData
                    required property int index
                    Layout.row: index
                    Layout.column: 0
                    text: dialog.mode === "headers"
                        ? "Field " + (index + 1) + ": " + (((dialog.preview.rows || [])[0] || { fields: [] }).fields[index] || "")
                        : modelData
                    elide: Text.ElideRight
                    Layout.maximumWidth: 260
                }
            }
            Repeater {
                model: dialog.fieldKeys
                delegate: AppComboBox {
                    required property string modelData
                    required property int index
                    objectName: "role_" + modelData
                    Layout.row: index
                    Layout.column: 1
                    Accessible.name: (dialog.mode === "headers" ? "Field " + (index + 1) : modelData) + " holds"
                    model: dialog.roleNames.map(r => r[1])
                    currentIndex: Math.max(0, dialog.roleNames.findIndex(r => r[0] === (dialog.roles[modelData] || "ignore")))
                    onActivated: i => dialog.setRole(modelData, dialog.roleNames[i][0])
                }
            }
        }
        AppCheckBox {
            id: createSpecimens
            objectName: "createSpecimens"
            text: "Add specimens the table does not have yet"
            checked: true
        }
        AppCheckBox {
            id: overwrite
            objectName: "overwriteValues"
            text: "Replace values and links that differ (otherwise they are kept and listed)"
        }
        AppErrorLabel {
            visible: text.length > 0
            text: dialog.message || (dialog.fieldKeys.length && !dialog.hasSpecimen ? "Choose which field holds the specimen ID" : "")
            color: "#c0392b"
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        AppButton {
            id: planButton
            objectName: "planImport"
            text: "Review changes"
            enabled: dialog.hasSpecimen && !dialog.preview.error
            onClicked: dialog.planRequested(dialog.separator, dialog.roleList(), createSpecimens.checked, overwrite.checked)
        }
    }
}
