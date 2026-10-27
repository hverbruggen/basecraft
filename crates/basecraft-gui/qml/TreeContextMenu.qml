import QtQuick
import QtQuick.Controls.Basic

Menu {
    id: menu
    property var controller: null
    property var info: ({})
    property var annotation: null
    property string revision: ""
    property string tipName: ""
    signal nameRequested(var target, var annotation, string revision)
    signal renameTipRequested(var target, string name, string revision)
    signal rootPositionRequested()
    signal supportRequested()
    signal styleRequested()

    function showFor(target, context) {
        controller = target
        info = context
        revision = target.record_revision()
        annotation = context.node ? JSON.parse(target.annotation_json()) : null
        if (annotation && annotation.error) {
            target.message_kind = "error"
            target.message = annotation.error
            annotation = null
        }
        tipName = context.tip ? target.tip_name() : ""
        popup()
    }

    function validTarget() {
        if (controller && revision.length && controller.record_revision() === revision) return true
        if (controller) {
            controller.message_kind = "error"
            controller.message = "The tree changed. Open the context menu again."
        }
        return false
    }

    component TreeItem: MenuItem { height: visible ? implicitHeight : 0 }
    TreeItem {
        objectName: "nameAnnotation"
        text: menu.info.annotated ? (menu.info.tip ? "Edit tip annotation…" : "Edit clade annotation…")
            : menu.info.tip ? "Annotate tip…" : "Annotate clade…"
        visible: menu.info.node === true
        enabled: menu.annotation !== null
        onTriggered: if (menu.validTarget()) menu.nameRequested(menu.controller, menu.annotation, menu.revision)
    }
    TreeItem {
        text: "Remove annotation"
        visible: menu.info.annotated === true
        onTriggered: if (menu.validTarget()) menu.controller.remove_annotation(menu.info.annotation)
    }
    TreeItem {
        objectName: "createCladeAlignment"
        text: "Create clade alignment"
        visible: menu.info.node === true
        enabled: menu.controller !== null && !menu.controller.busy && menu.controller.has_selection === true
        onTriggered: if (menu.validTarget()) menu.controller.extract_clade_alignment()
    }
    TreeItem { text: "Reroot here"; visible: menu.info.node === true && !menu.info.root; onTriggered: if (menu.validTarget()) menu.controller.reroot() }
    TreeItem { text: "Root position…"; visible: menu.info.rootBranch === true; onTriggered: if (menu.validTarget()) menu.rootPositionRequested() }
    TreeItem { text: "Flip subtree"; visible: menu.info.node === true; onTriggered: if (menu.validTarget()) menu.controller.rotate() }
    TreeItem {
        objectName: "renameTipIdentity"
        text: "Rename tip identity…"
        visible: menu.info.tip === true
        onTriggered: if (menu.validTarget()) menu.renameTipRequested(menu.controller, menu.tipName, menu.revision)
    }
    TreeItem { text: "Add support from tree…"; visible: !menu.info.node; onTriggered: menu.supportRequested() }
    TreeItem { text: "Tree style…"; visible: !menu.info.node; onTriggered: menu.styleRequested() }
}
