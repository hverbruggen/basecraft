import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

RibbonGroup {
    id: group
    title: "Linked alignment"
    property var controller: null
    property var customization: ({shortcuts: {}, ribbon_hidden: []})
    readonly property string linkedPath: controller ? controller.linked_alignment : ""
    readonly property string linkStatus: controller ? (controller.alignment_link_status || "") : ""
    readonly property string sourcePath: controller ? (controller.source_alignment || "") : ""
    readonly property var commands: [link.commandAction, extract.commandAction]
    signal linkRequested(var target)
    AppCommandButton {
        id: link
        objectName: "linkTreeAlignment"
        commandId: "ribbon.associate-an-imported-tree-with-an-alignment-after-validating"
        customization: group.customization
        text: group.linkedPath.length ? "Change alignment" : "Link alignment"
        iconName: "align"
        tip: (group.linkStatus.length ? group.linkStatus + "\n" : "")
            + (group.linkedPath.length ? "Linked alignment: " + group.linkedPath + "\n" : "")
            + (group.sourcePath.length ? "Original analysis source: " + group.sourcePath + "\n" : "")
            + "Choose an alignment for viewing and selection. Identifiers are validated; the original analysis source is retained."
        enabled: group.controller !== null && !group.controller.busy
        onInvoked: group.linkRequested(group.controller)
    }
    AppCommandButton {
        id: extract
        objectName: "openCladeAlignment"
        commandId: "ribbon.extract-aligned-rows-without-realigning-ctrl-click-to-select"
        customization: group.customization
        text: "Open selected clade(s)"
        iconName: "selection"
        tip: "Extract aligned rows without realigning; Ctrl-click to select multiple clades"
        enabled: group.controller !== null && !group.controller.busy
        onInvoked: group.controller.extract_clade_alignment()
    }
}
