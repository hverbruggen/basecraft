import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

AppFormDialog {
    id: dialog
    required property string kind
    property var target: null
    title: kind === "matrix" ? "Distance matrix" : kind === "nj" ? "Neighbor joining" : "UPGMA"
    initialFocusItem: distanceModel
    standardButtons: Dialog.Ok | Dialog.Cancel
    onOpened: standardButton(Dialog.Ok).text = kind === "matrix" ? "Calculate" : "Build tree"
    onAccepted: if (target && !target.busy) target.distance_action(kind, JSON.stringify(parameters()))
    function parameters() {
        const p = {model: distanceModel.currentText, min_overlap: distanceOverlap.value}
        if (kind === "nj") p.method = distanceBootstrap.checked ? "sorted" : distanceMethod.currentText
        if (kind !== "matrix" && distanceBootstrap.checked) {
            p.bootstrap = bootstrapReplicates.value
            p.seed = bootstrapSeed.value
        }
        return p
    }
    AppComboBox { objectName: "distanceModel"; Kirigami.FormData.label: "DNA model"; id: distanceModel; model: ["p", "jc69", "k2p"]; currentIndex: dialog.kind === "matrix" ? 0 : 2 }
    SpinBox { Kirigami.FormData.label: "Minimum shared sites"; id: distanceOverlap; from: 1; to: 100000000; value: 1; editable: true }
    AppComboBox { objectName: "distanceMethod"; Kirigami.FormData.label: "Missing-distance method"; id: distanceMethod; model: ["njstar", "sorted", "fourpoint", "mvrstar"]; visible: dialog.kind === "nj"; enabled: !distanceBootstrap.checked }
    CheckBox { objectName: "distanceBootstrap"; id: distanceBootstrap; Kirigami.FormData.label: "Support"; text: "Bootstrap support"; visible: dialog.kind !== "matrix"; onCheckedChanged: if (checked && dialog.kind === "nj") distanceMethod.currentIndex = 1 }
    SpinBox { Kirigami.FormData.label: "Bootstrap replicates"; id: bootstrapReplicates; from: 1; to: 1000000; value: 1000; editable: true; visible: dialog.kind !== "matrix"; enabled: distanceBootstrap.checked }
    SpinBox { Kirigami.FormData.label: "Bootstrap seed"; id: bootstrapSeed; from: 0; to: 2147483647; value: 1; editable: true; visible: dialog.kind !== "matrix"; enabled: distanceBootstrap.checked }
    Label { Kirigami.FormData.isSection: true; Layout.fillWidth: true; wrapMode: Text.Wrap; visible: dialog.kind !== "matrix"; text: "Bootstrap uses complete distances; any failed replicate withholds support." }
    Label { Kirigami.FormData.isSection: true; Layout.fillWidth: true; wrapMode: Text.Wrap; text: (dialog.kind === "upgma" ? "UPGMA requires complete distances within each connected group.\n" : "") + "Gaps and ambiguous residues are excluded pairwise." }
}
