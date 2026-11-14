import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Predict protein-coding genes with Prodigal in the open sequence or its selection.
AppContentDialog {
    id: genesDialogItem
    required property var app
    initialFocusItem: codeBox
    property bool inSelection: false
    property var source: null
    property string sourcePath: ""
    function predict(selectionOnly) {
        if (!app.current || app.current.busy) return
        source = app.current
        sourcePath = source.file_path
        inSelection = selectionOnly
        // Where the record states no code, Prodigal's usual inputs
        // (bacteria, archaea, plastids) use table 11, not the standard
        // code: its start rule differs.
        const stated = source.genetic_code_source() !== "default"
        const code = stated ? source.genetic_code() : 11
        let i = -1
        for (let k = 0; k < geneticCodes.count; k++)
            if (geneticCodes.get(k).code === code) i = k
        codeBox.currentIndex = i
        suggested = i
        suggestedFrom = stated ? source.genetic_code_source() : "usual for bacteria, archaea and plastids"
        open()
    }
    property int suggested: -1
    property string suggestedFrom: ""
    title: inSelection ? "Predict genes in the selection" : "Predict genes"
    standardButtons: Dialog.Ok | Dialog.Cancel
    onOpened: codeBox.forceActiveFocus()
    onAccepted: if (codeBox.currentIndex >= 0 && source && source.file_path === sourcePath && !source.busy) source.predict_genes(geneticCodes.get(codeBox.currentIndex).code, inSelection)
    body: ColumnLayout {
        spacing: 8
        RowLayout {
            Label { id: codeBoxLabel; text: "Genetic code" }
            AppComboBox {
                id: codeBox; objectName: "codeBox"; Accessible.labelledBy: codeBoxLabel; Accessible.name: codeBoxLabel.text
                Layout.fillWidth: true
                model: geneticCodes
                textRole: "label"
                Keys.onReturnPressed: genesDialogItem.accept()
                Keys.onEnterPressed: genesDialogItem.accept()
            }
            Label {
                text: codeBox.currentIndex === genesDialogItem.suggested ? "(" + genesDialogItem.suggestedFrom + ")" : ""
                opacity: 0.7
            }
        }
        Label {
            text: "Prodigal trains on sequences of 20 kb or more. Shorter ones use its preset models,\nwhich exist only for codes 4 and 11. Genes are added as CDS; one Undo removes them all."
            opacity: 0.7
        }
    }
    // NCBI genetic codes that Prodigal supports.
    ListModel {
        id: geneticCodes
        ListElement { code: 1; label: "1  Standard" }
        ListElement { code: 2; label: "2  Vertebrate mitochondrial" }
        ListElement { code: 3; label: "3  Yeast mitochondrial" }
        ListElement { code: 4; label: "4  Mold, protozoan and coelenterate mitochondrial; Mycoplasma" }
        ListElement { code: 5; label: "5  Invertebrate mitochondrial" }
        ListElement { code: 6; label: "6  Ciliate, dasycladacean and Hexamita nuclear" }
        ListElement { code: 9; label: "9  Echinoderm and flatworm mitochondrial" }
        ListElement { code: 10; label: "10  Euplotid nuclear" }
        ListElement { code: 11; label: "11  Bacterial, archaeal and plant plastid" }
        ListElement { code: 12; label: "12  Alternative yeast nuclear" }
        ListElement { code: 13; label: "13  Ascidian mitochondrial" }
        ListElement { code: 14; label: "14  Alternative flatworm mitochondrial" }
        ListElement { code: 15; label: "15  Blepharisma nuclear" }
        ListElement { code: 16; label: "16  Chlorophycean mitochondrial" }
        ListElement { code: 21; label: "21  Trematode mitochondrial" }
        ListElement { code: 22; label: "22  Scenedesmus obliquus mitochondrial" }
        ListElement { code: 23; label: "23  Thraustochytrium mitochondrial" }
        ListElement { code: 24; label: "24  Rhabdopleuridae mitochondrial" }
        ListElement { code: 25; label: "25  Candidate division SR1 and Gracilibacteria" }
        ListElement { code: 26; label: "26  Pachysolen tannophilus nuclear" }
        ListElement { code: 29; label: "29  Mesodinium nuclear" }
        ListElement { code: 30; label: "30  Peritrich nuclear" }
        ListElement { code: 32; label: "32  Balanophoraceae plastid" }
        ListElement { code: 33; label: "33  Cephalodiscidae mitochondrial" }
    }
}
