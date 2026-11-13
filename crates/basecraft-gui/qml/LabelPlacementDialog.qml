import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// After a reroot leaves clade labels on other groups of tips (D11), ask where
// each goes. The tree controller decides what is offered and applies the
// answer as one undo step. Escape or Cancel leaves the labels where they are.
AppContentDialog {
    id: dialog
    objectName: "labelPlacementDialog"
    property var controller: null
    readonly property var question: controller && controller.label_question.length ? JSON.parse(controller.label_question) : null
    onQuestionChanged: question ? open() : close()
    title: "Place clade label"
    standardButtons: Dialog.Cancel
    initialFocusItem: exact.visible ? exact : smallest
    onRejected: if (controller) controller.dismiss_labels()
    function choose(answer) { dialog.controller.answer_label(answer, all.checked) }
    body: ColumnLayout {
        spacing: 6
        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: dialog.question ? "The reroot left “" + dialog.question.label + "” on a group of tips it was not made for (label " + dialog.question.number + " of " + dialog.question.total + ")." : ""
        }
        AppCheckBox { id: all; objectName: "labelPlacementAll"; text: "&Apply to all remaining labels"; visible: !!dialog.question && dialog.question.number < dialog.question.total }
        AppButton { id: exact; objectName: "labelExact"; text: "Move to the clade with &exactly its tips"; visible: !!dialog.question && (all.checked ? dialog.question.exact_for_all : dialog.question.exact); onClicked: dialog.choose("exact") }
        AppButton { id: smallest; objectName: "labelSmallest"; text: "Move to the &smallest clade holding its tips"; onClicked: dialog.choose("smallest") }
        AppButton { objectName: "labelRemove"; text: "&Remove label"; onClicked: dialog.choose("remove") }
        AppButton { objectName: "labelUndo"; text: "&Undo reroot"; onClicked: dialog.choose("undo") }
    }
}
