import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Choose which hypotheses of each file to combine. Each analysis run lists its
// best partition first; ranked alternatives start unchecked.
AppContentDialog {
    id: dialog
    title: "Choose hypotheses to combine"
    standardButtons: Dialog.Ok | Dialog.Cancel
    // [{path, file, specimens, hypotheses: [{id, name, code, method, species,
    //   unassigned, best, parent, scores: [{value, scale}]}]}]
    property var files: []
    // Path to the list of checked IDs.
    property var chosen: ({})
    readonly property int count: {
        let n = 0
        for (const path in chosen) n += chosen[path].length
        return n
    }
    signal chosenHypotheses(string selection)
    function alternative(h) { return !h.best && (h.code === "A" || h.code === "G") }
    function show(candidates) {
        files = candidates
        chosen = ({})
        for (const file of candidates)
            for (const h of file.hypotheses)
                if (!alternative(h)) toggle(file, h, true)
        open()
    }
    function isChosen(path, id) { return (chosen[path] || []).includes(id) }
    // Edited copies need their source: checking a copy checks its source, and
    // unchecking a source unchecks its copies.
    function toggle(file, h, on) {
        let ids = (chosen[file.path] || []).slice()
        const add = id => { if (!ids.includes(id)) ids.push(id) }
        if (on) {
            for (let next = h; next; next = file.hypotheses.find(x => x.id === next.parent)) add(next.id)
        } else {
            const drop = new Set([h.id])
            let grew = true
            while (grew) {
                grew = false
                for (const x of file.hypotheses)
                    if (x.parent && drop.has(x.parent) && !drop.has(x.id)) { drop.add(x.id); grew = true }
            }
            ids = ids.filter(id => !drop.has(id))
        }
        const next = Object.assign({}, chosen)
        next[file.path] = ids
        chosen = next
    }
    function describe(h) {
        const parts = [h.species + " species"]
        if (h.unassigned) parts.push(h.unassigned + " unassigned")
        for (const s of h.scores) parts.push(s.scale + " " + Number(s.value.toPrecision(4)))
        return parts.join(", ")
    }
    onOpened: standardButton(Dialog.Ok).enabled = Qt.binding(() => dialog.count > 0)
    onAccepted: chosenHypotheses(JSON.stringify(chosen))

    body: ColumnLayout {
        spacing: 12
        Label {
            text: "Best result of each analysis first. Ranked alternatives start unchecked."
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        Repeater {
            model: dialog.files
            delegate: ColumnLayout {
                id: fileBlock
                required property var modelData
                Layout.fillWidth: true
                spacing: 2
                Label {
                    text: fileBlock.modelData.file + " · " + fileBlock.modelData.specimens + " specimens"
                    font.bold: true
                    elide: Text.ElideMiddle
                    Layout.fillWidth: true
                }
                Repeater {
                    model: fileBlock.modelData.hypotheses
                    delegate: RowLayout {
                        id: row
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: 6
                        AppCheckBox {
                            id: option
                            objectName: "combine-" + row.modelData.id
                            padding: 0
                            Layout.alignment: Qt.AlignTop
                            Layout.topMargin: 1
                            checked: dialog.isChosen(fileBlock.modelData.path, row.modelData.id)
                            onToggled: {
                                dialog.toggle(fileBlock.modelData, row.modelData, checked)
                                // A click replaces the binding; restore it so linked copies stay in step.
                                checked = Qt.binding(() => dialog.isChosen(fileBlock.modelData.path, row.modelData.id))
                            }
                            Accessible.name: row.modelData.name + ", " + row.modelData.method + ", " + dialog.describe(row.modelData) + (row.modelData.best ? ", best" : "")
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            Label {
                                text: row.modelData.code + "  " + row.modelData.name + (row.modelData.best ? "  (best)" : "")
                                font.bold: row.modelData.best
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            Label {
                                text: row.modelData.method + " · " + dialog.describe(row.modelData)
                                opacity: 0.75
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            // Clicking the text toggles the box, as on a labelled checkbox.
                            TapHandler { onTapped: { option.toggle(); option.toggled() } }
                        }
                    }
                }
            }
        }
    }
}
