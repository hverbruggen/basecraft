import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// D12: basecraft cannot tell whether a document is DNA, RNA or protein. Ask
// once when it opens; the answer is recorded as stated by the user. Cancel
// leaves it undetermined, and tools that need to know refuse and say why.
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
        AppButton { id: dna; objectName: "alphabetDna"; text: "It is &DNA"; onClicked: dialog.controller.state_alphabet("dna") }
        AppButton { objectName: "alphabetRna"; text: "It is &RNA"; onClicked: dialog.controller.state_alphabet("rna") }
        AppButton { objectName: "alphabetProtein"; text: "It is &protein"; onClicked: dialog.controller.state_alphabet("protein") }
    }
}
