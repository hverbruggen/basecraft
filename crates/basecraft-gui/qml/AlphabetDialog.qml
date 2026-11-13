import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// D12, D22: basecraft cannot tell whether a document is DNA, RNA or protein.
// Ask when it opens; the answer goes to the side file at once. Cancel means
// not now: no question until the file is opened again. "Don't ask again"
// records it as undetermined by the user; tools that need to know still
// refuse and say why.
AppContentDialog {
    id: dialog
    objectName: "alphabetDialog"
    property var controller: null
    readonly property string question: controller && controller.alphabet_question !== undefined ? controller.alphabet_question : ""
    onQuestionChanged: question.length ? open() : close()
    title: "Sequence type"
    standardButtons: Dialog.Cancel
    initialFocusItem: dna
    onRejected: if (controller) controller.alphabet_question = ""
    body: ColumnLayout {
        spacing: 6
        Label { Layout.fillWidth: true; wrapMode: Text.Wrap; text: dialog.question }
        AppButton { id: dna; objectName: "alphabetDna"; text: "Mark as &DNA"; onClicked: dialog.controller.state_alphabet("dna") }
        AppButton { objectName: "alphabetRna"; text: "Mark as &RNA"; onClicked: dialog.controller.state_alphabet("rna") }
        AppButton { objectName: "alphabetProtein"; text: "Mark as &protein"; onClicked: dialog.controller.state_alphabet("protein") }
        AppButton { objectName: "alphabetDecline"; text: "Do&n't ask again for this file"; onClicked: dialog.controller.state_alphabet("undetermined") }
    }
}
