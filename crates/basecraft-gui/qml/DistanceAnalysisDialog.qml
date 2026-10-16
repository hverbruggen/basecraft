import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppContentDialog {
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
            p.bootstrap_destination = bootstrapDestination.currentIndex === 0 ? "metadata" : "working"
        }
        return p
    }
    body: ColumnLayout {
        spacing: 10
        AppFormRow {
            objectName: "distanceSettings"
            ColumnLayout {
                Layout.minimumWidth: 0
                Label {
                    id: modelLabel
                    text: "DNA model"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                AppComboBox {
                    id: distanceModel
                    objectName: "distanceModel"
                    Accessible.labelledBy: modelLabel
                    Accessible.name: modelLabel.text
                    model: ["p", "jc69", "k2p"]
                    currentIndex: dialog.kind === "matrix" ? 0 : 2
                }
            }
            ColumnLayout {
                Layout.minimumWidth: 0
                Label {
                    id: overlapLabel
                    text: "Minimum shared sites"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                AppSpinBox {
                    id: distanceOverlap
                    objectName: "distanceOverlap"
                    Accessible.labelledBy: overlapLabel
                    Accessible.name: overlapLabel.text
                    from: 1
                    to: 100000000
                    value: 1
                    editable: true
                }
            }
        }
        ColumnLayout {
            visible: dialog.kind === "nj"
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            Label {
                id: methodLabel
                text: "Missing-distance method"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            AppComboBox {
                id: distanceMethod
                objectName: "distanceMethod"
                Accessible.labelledBy: methodLabel
                Accessible.name: methodLabel.text
                model: ["njstar", "sorted", "fourpoint", "mvrstar"]
                enabled: !distanceBootstrap.checked
            }
        }
        AppFormRow {
            objectName: "bootstrapSettings"
            visible: dialog.kind !== "matrix"
            AppCheckBox {
                id: distanceBootstrap
                objectName: "distanceBootstrap"
                Layout.minimumWidth: 0
                text: "Bootstrap"
                Accessible.name: "Bootstrap support"
                onCheckedChanged: if (checked && dialog.kind === "nj")
                    distanceMethod.currentIndex = 1
            }
            ColumnLayout {
                enabled: distanceBootstrap.checked
                Layout.minimumWidth: 0
                Label {
                    id: replicatesLabel
                    text: "Replicates"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                AppSpinBox {
                    id: bootstrapReplicates
                    objectName: "bootstrapReplicates"
                    Accessible.labelledBy: replicatesLabel
                    Accessible.name: "Bootstrap " + replicatesLabel.text.toLowerCase()
                    from: 1
                    to: 1000000
                    value: 1000
                    editable: true
                }
            }
            ColumnLayout {
                enabled: distanceBootstrap.checked
                Layout.minimumWidth: 0
                Label {
                    id: seedLabel
                    text: "Seed"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                AppSpinBox {
                    id: bootstrapSeed
                    objectName: "bootstrapSeed"
                    Accessible.labelledBy: seedLabel
                    Accessible.name: "Bootstrap " + seedLabel.text.toLowerCase()
                    from: 0
                    to: 2147483647
                    value: 1
                    editable: true
                }
            }
        }
        ColumnLayout {
            visible: dialog.kind !== "matrix" && distanceBootstrap.checked
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            Label {
                id: destinationLabel
                text: "Save bootstrap trees in"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            AppComboBox {
                id: bootstrapDestination
                objectName: "bootstrapDestination"
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                Accessible.labelledBy: destinationLabel
                Accessible.name: destinationLabel.text
                model: ["Metadata directory (.basecraft/)", "Working directory"]
                currentIndex: 0
            }
        }
        Label {
            Layout.preferredWidth: 1
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            visible: dialog.kind !== "matrix"
            text: "Bootstrap uses complete distances; any failed replicate withholds support."
        }
        Label {
            Layout.preferredWidth: 1
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: (dialog.kind === "upgma" ? "UPGMA requires complete distances within each connected group.\n" : "") + "Gaps and ambiguous residues are excluded pairwise."
        }
    }
}
