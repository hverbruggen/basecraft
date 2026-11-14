import QtQuick
import QtQuick.Controls.Basic

// Only what applies where you clicked in a sequence: an annotation, or the selection.
Menu {
    id: menu
    required property var app
    required property var editor
    required property var clipboard
    required property var addAnnotationCommand
    required property var orfDialog
    required property var genesDialog
    property var info: ({ feature: -1, label: "", featureLength: 0, selection: false, length: 0 })
    readonly property bool onAnnotation: info.feature >= 0
    component Item: MenuItem { height: visible ? implicitHeight : 0 }

    Item { text: "Edit “" + menu.info.label + "”…"; visible: menu.onAnnotation; onTriggered: editor.openFeature(menu.info.feature) }
    Item { text: "Delete “" + menu.info.label + "”"; visible: menu.onAnnotation; onTriggered: app.current.delete_feature(menu.info.feature) }
    Item {
        text: "Copy annotation sequence (" + menu.info.featureLength + " bp)"
        visible: menu.onAnnotation
        onTriggered: clipboard.put(app.current.feature_sequence(menu.info.feature))
    }
    Item { text: "BLAST annotation at NCBI"; visible: menu.onAnnotation; onTriggered: app.beginBlast("annotation", menu.info.feature) }
    Item { text: "Extract as new sequence"; visible: menu.onAnnotation; onTriggered: app.current.extract_feature(menu.info.feature) }
    Item { text: "Zoom to annotation"; visible: menu.onAnnotation; onTriggered: app.current.zoom_to_feature(menu.info.feature) }

    Item { text: "Add annotation…"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: addAnnotationCommand.trigger() }
    Item {
        text: "Copy sequence (" + menu.info.length + " bp)"
        visible: !menu.onAnnotation
        enabled: menu.info.selection
        onTriggered: clipboard.put(app.current.selection_text())
    }
    Item { text: "Extract as new sequence"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: app.current.extract_selection() }
    Item { text: "Zoom to selection"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: app.current.zoom_to_selection() }
    Item { text: "BLAST selection at NCBI"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: app.beginBlast("selection") }
    Item { text: "Find ORFs in selection…"; visible: !menu.onAnnotation; enabled: menu.info.selection && app.current && !app.current.busy; onTriggered: orfDialog.dialog().search(true) }
    Item { text: "Predict genes in selection…"; visible: !menu.onAnnotation; enabled: menu.info.selection && app.current && !app.current.busy; onTriggered: genesDialog.dialog().predict(true) }
}
