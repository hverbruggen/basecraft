import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppContentDialog {
    id: dialog
    property var controller: null
    property var editController: null
    property string editRevision: ""
    property string oldName: ""
    initialFocusItem: tipName
    title: "Rename tip identity"

    function ask() {
        if (!controller || !controller.tip_selected) return
        openCaptured(controller, controller.tip_name(), controller.record_revision())
    }
    function openCaptured(target, name, revision) {
        editController = target
        editRevision = revision
        oldName = name
        tipName.text = name
        tipError.text = ""
        open()
    }
    function save() {
        if (!editController || !editRevision.length || editController.record_revision() !== editRevision
                || editController.tip_name() !== oldName) {
            tipError.text = "The tree or selected tip changed. Close this editor and choose the tip again."
            return
        }
        if (tipName.text.trim() !== oldName && (editController.linked_alignment
                || editController.source_alignment || editController.alignment_link_inferred)) {
            tipError.text = "This tree is linked to an alignment. Use a tip annotation to change its displayed name while keeping the linked identity."
            return
        }
        const identity = JSON.parse(editController.annotation_json())
        if (!identity || identity.error) {
            tipError.text = identity && identity.error ? identity.error : "Choose an unambiguous tip first."
            return
        }
        const err = editController.rename_tip(tipName.text)
        if (err.length > 0) tipError.text = err
        else close()
    }
    onOpened: { tipName.forceActiveFocus(); tipName.selectAll() }
    body: ColumnLayout {
        spacing: 6
        Label { id: tipNameLabel; text: "Tip identity" }
        TextField {
            id: tipName
            objectName: "tipName"
            Accessible.labelledBy: tipNameLabel
            Accessible.name: tipNameLabel.text
            Layout.preferredWidth: 320
            Layout.fillWidth: true
            Keys.onReturnPressed: dialog.save()
            Keys.onEnterPressed: dialog.save()
        }
        AppErrorLabel { id: tipError; objectName: "tipNameError"; visible: text.length > 0; wrapMode: Text.Wrap; Layout.fillWidth: true }
        RowLayout {
            Item { Layout.fillWidth: true }
            AppButton { text: "Cancel"; onClicked: dialog.reject() }
            AppButton { text: "Rename"; highlighted: true; onClicked: dialog.save() }
        }
    }
}
