import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Sequence: annotations, their sequences and searches.
RibbonPage {
    required property var app
    required property var addAnnotationCommand
    required property var editor
    required property var clipboard
    required property var orfDialog
    required property var genesDialog
    spacing: 6 * app.ribbonSizeFactor
    RibbonGroup {
        disabledReason: "Open a sequence to use this group"
        enabled: app.currentKind === "sequence"
        title: "Annotation"
        AppRibbonButton { action: addAnnotationCommand; text: app.commandList.ribbon_label(addAnnotationCommand.commandId); visible: addAnnotationCommand.visible; iconName: "annotation-add"; tip: addAnnotationCommand.effectiveTooltip }
        RibbonButton { commandId: "ribbon.edit-the-selected-annotation"; iconName: "annotation-edit"; keys: "Enter"; onInvoked: editor.openSelected() }
        RibbonButton { commandId: "ribbon.delete-the-selected-annotation";
            iconName: "annotation-delete"
            keys: "Delete"
            onInvoked: {
                const id = app.current.selected_feature()
                if (id >= 0) app.current.delete_feature(id)
                else { app.current.message_kind = ""; app.current.message = "Click an annotation first" }
            }
        }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        disabledReason: "Open a sequence to use this group"
        enabled: app.currentKind === "sequence"
        title: "Go to"
        RibbonButton { commandId: "ribbon.select-the-previous-annotation"; iconName: "left"; onInvoked: app.current.step_feature(false) }
        RibbonButton { commandId: "ribbon.select-the-next-annotation"; iconName: "right"; onInvoked: app.current.step_feature(true) }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        disabledReason: "Open a sequence to use this group"
        enabled: app.currentKind === "sequence"
        title: "Sequence"
        RibbonButton { commandId: "ribbon.copy-the-selected-annotation-s-sequence";
            text: "Copy"
            tip: "Copy the selected annotation's sequence"
            keys: "Ctrl+C"
            onInvoked: {
                const id = app.current.selected_feature()
                if (id >= 0) clipboard.put(app.current.feature_sequence(id))
                else { app.current.message_kind = ""; app.current.message = "Click an annotation first" }
            }
        }
        RibbonButton { commandId: "ribbon.save-the-selection-as-a-new-file"; text: "Extract"; tip: "Save the selection as a new file"; keys: "Ctrl+E"; onInvoked: app.current.extract_selection() }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        disabledReason: "Open a sequence to use this group"
        enabled: app.currentKind === "sequence"
        title: "Find"
        RibbonButton { commandId: "sequence.blast"; iconName: "search"; onInvoked: app.beginBlast("whole") }
        RibbonButton { commandId: "ribbon.annotate-open-reading-frames";
            text: "ORFs"
            tip: "Annotate open reading frames"
            enabled: app.current && !app.current.busy
            onInvoked: orfDialog.dialog().search(false)
        }
        RibbonButton { commandId: "ribbon.annotate-protein-coding-genes-with-prodigal";
            text: "Prodigal"
            tip: "Annotate protein-coding genes with Prodigal"
            enabled: app.current && !app.current.busy
            onInvoked: genesDialog.dialog().predict(false)
        }
    }
    Item { Layout.fillWidth: true }
    PageHelpGroup {}
}
