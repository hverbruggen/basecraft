import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Basecraft

FocusScope {
    id: doc
    property var notificationSink: null
    Connections {
        target: ctl
        function onMessageChanged() {
            if (notificationSink && ctl.message.length)
                notificationSink.notification(ctl.file_path, ctl.message, ctl.message_kind)
        }
    }

    required property string path
    required property int index
    property alias controller: ctl
    property alias view: treeCanvas
    readonly property bool dirty: ctl.dirty
    readonly property string kind: "delimitation"
    readonly property var workspace: JSON.parse(ctl.workspace_json)
    readonly property var selection: JSON.parse(ctl.selection_json)
    property string tool: "select"
    property var mergeGroups: []
    property bool settingsOpen: false
    property bool detailsOpen: false
    property var pendingDisplay: null
    // Created by an analysis request; discarded on close if nothing came of it.
    property bool draft: false
    property bool showGroups: !ctl.display_path.length
    property alias treeController: tree
    function startAnalysis(method, input, rooted, settings, displayMethod) {
        if (ctl.busy) return
        if (method === "manual" || method === "view") {
            if (method === "manual") {
            ctl.action("create", JSON.stringify({name: "Manual species"}))
            const hypotheses = JSON.parse(ctl.workspace_json).hypotheses || []
            if (hypotheses.length) ctl.activate(hypotheses[hypotheses.length - 1].id)
            }
            if (["upgma", "nj", "iqtree"].includes(displayMethod)) {
                showGroups = false
                ctl.run_analysis("display_" + displayMethod, input, false, settings)
            } else if (displayMethod === "existing") { ctl.attach_tree(input, rooted); showGroups = false }
            else showGroups = true
            return
        }
        // The display tree only illustrates ASAP's result, so build it after
        // ASAP succeeds; a failed run then leaves no tree behind.
        if (method === "asap" && (["upgma", "nj", "iqtree"].includes(displayMethod)))
            pendingDisplay = {method: "display_" + displayMethod, input: input, settings: settings}
        else
            showGroups = displayMethod === "none"
        ctl.run_analysis(method, input, rooted, settings)
    }
    Connections {
        target: ctl
        function onDisplayTreeReady(success) {
            if (success) doc.showGroups = false
        }
        function onAnalysisFinished(success) {
            const pending = doc.pendingDisplay
            doc.pendingDisplay = null
            if (success && pending) ctl.run_analysis(pending.method, pending.input, false, pending.settings)
        }
    }
    component WorkspaceButton: AppButton {
        id: workspaceButton
        FontMetrics { id: buttonMetrics; font: workspaceButton.font }
        padding: 6
        implicitHeight: Math.max(32, implicitContentHeight + topPadding + bottomPadding)
        implicitWidth: Math.max(50, buttonMetrics.advanceWidth(text) + leftPadding + rightPadding)
    }
    Timer { interval: 250; running: ctl.busy; repeat: true; onTriggered: ctl.poll_job() }
    signal openFile(string path)
    DelimitationController { id: ctl }
    TreeController { id: tree }
    Component.onCompleted: ctl.open_file(path)
    Connections { target: ctl
        function onDisplay_pathChanged() { if (ctl.display_path.length) tree.open_file(ctl.display_path) }
        function onDisplayTaxaSelected(names) { tree.highlight_taxa(names) }
    }
    Connections { target: tree
        function onTaxaSelected(names) { ctl.tree_taxa(names) }
    }
    ColumnLayout {
        anchors.fill: parent
        spacing: 4
        Flow {
            enabled: !ctl.busy
            Layout.fillWidth: true
            Layout.preferredHeight: childrenRect.height
            spacing: 4
            TextField { id: name; width: Math.max(170, implicitWidth); Accessible.name: "Hypothesis / group name"; placeholderText: "Hypothesis / group name" }
            WorkspaceButton { text: "New"; onClicked: ctl.action("create", JSON.stringify({name:name.text})) }
            WorkspaceButton { text: "Edit copy"; enabled: ctl.active_hypothesis.length > 0; onClicked: ctl.action("derive", JSON.stringify({name:name.text})) }
            WorkspaceButton { text: "Rename"; onClicked: ctl.action("rename",JSON.stringify({name:name.text})) }
            WorkspaceButton { text: "Group selection"; enabled: doc.selection.length > 0; onClicked: ctl.action("group",JSON.stringify({label:name.text})) }
            WorkspaceButton { text: "Merge checked"; enabled: doc.mergeGroups.length > 1; onClicked: { ctl.action("merge",JSON.stringify({groups:doc.mergeGroups,label:name.text})); doc.mergeGroups = [] } }
            WorkspaceButton { text: "Unassign"; enabled: doc.selection.length > 0; onClicked: ctl.action("unassign","{}") }
            WorkspaceButton { text: "Select"; checkable: true; checked: doc.tool === "select"; onClicked: { doc.tool = "select"; ctl.cancel_cut() } }
            WorkspaceButton { text: "Cut"; checkable: true; checked: doc.tool === "cut"; enabled: ctl.can_cut; onClicked: doc.tool = "cut" }
            WorkspaceButton { text: "SVG"; onClicked: ctl.export_svg(doc.path + ".svg") }
            WorkspaceButton { text: "Details"; checkable: true; checked: doc.detailsOpen; onClicked: doc.detailsOpen = !doc.detailsOpen }
            WorkspaceButton { text: doc.showGroups ? "Tree view" : "Sequence groups"; enabled: ctl.display_path.length > 0; onClicked: doc.showGroups = !doc.showGroups }
            WorkspaceButton { text: doc.settingsOpen ? "Hide advanced tools" : "Advanced tools"; onClicked: doc.settingsOpen = !doc.settingsOpen }
        }
        AppMessageBar {
            Layout.fillWidth: true
            message: ctl.message
            kind: ctl.message_kind
            details: ctl.message_details
            busy: ctl.busy
            onDetailsRequested: detailsDialog.open()
            onCancelRequested: ctl.cancel_job()
        }
        ScrollView {
            visible: doc.settingsOpen
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            Layout.preferredHeight: 310
            contentWidth: Math.max(doc.width, 1200)
            ColumnLayout {
                width: Math.max(doc.width, 1200)
        RowLayout {
            Layout.minimumWidth: 0
            visible: doc.settingsOpen
            TextField { id: treePath; placeholderText: "Display tree path"; Layout.fillWidth: true; text: ctl.display_path }
            CheckBox { id: rooted; text: "Explicitly rooted" }
            WorkspaceButton { text: "Attach tree"; onClicked: ctl.attach_tree(treePath.text,rooted.checked) }
            WorkspaceButton { text: "Select"; checkable: true; checked: doc.tool === "select"; onClicked: { doc.tool = "select"; ctl.cancel_cut() } }
            WorkspaceButton { text: "Cut"; checkable: true; checked: doc.tool === "cut"; enabled: ctl.can_cut; onClicked: doc.tool = "cut" }
            WorkspaceButton { text: "SVG"; onClicked: ctl.export_svg(doc.path + ".svg") }
        }
        RowLayout {
            visible: doc.tool === "cut"
            Label { text: "Manual time cut (age ≤ threshold)" }
            Slider { id: cutSlider; Layout.fillWidth: true; from: 0; to: ctl.root_height; value: ctl.threshold; onMoved: ctl.preview_cut(value) }
            TextField { id: cutValue; text: ctl.threshold.toPrecision(6); Layout.preferredWidth: 110; validator: DoubleValidator { bottom: 0; top: ctl.root_height } onEditingFinished: ctl.preview_cut(Number(text)) }
            Label { text: JSON.parse(ctl.preview_json).length + " preview groups" }
            WorkspaceButton { text: "Save cut"; enabled: JSON.parse(ctl.preview_json).length > 0; onClicked: ctl.save_cut(name.text.length ? name.text : "Manual time cut") }
            WorkspaceButton { text: "Cancel"; onClicked: { ctl.cancel_cut(); doc.tool = "select" } }
        }
        RowLayout {
            Layout.minimumWidth: 0
            visible: doc.settingsOpen
            TextField { id: analysisInput; placeholderText: "Immutable analysis input (tree or aligned FASTA)"; Layout.fillWidth: true; text: ["asap","haplotypes","group_statistics","gmyc_alignment_single","gmyc_alignment_multiple"].includes(method.currentText) ? ctl.linked_alignment : ctl.display_path }
            AppComboBox { id: method; model: ["gmyc_single", "gmyc_multiple", "gmyc_alignment_single", "gmyc_alignment_multiple", "ptp", "mptp", "asap", "relative_time", "haplotypes", "group_statistics"] }
            WorkspaceButton { text: "Run"; enabled: !ctl.busy; onClicked: ctl.run_analysis(method.currentText,analysisInput.text,rooted.checked,JSON.stringify({cache:cachePath.text,mapping:mappingPath.text,deduplicate:sampling.currentIndex === 0,model:distanceModel.currentText,seed:seed.value,workers:workers.value,replicates:replicates.value,tree_index:treeIndex.value})) }
            WorkspaceButton { text: "Cancel job"; enabled: ctl.busy; onClicked: ctl.cancel_job() }
        }
        RowLayout {
            Layout.minimumWidth: 0
            visible: doc.settingsOpen
            AppComboBox { id: sampling; model: ["Deduplicate exact aligned DNA", "All specimens"]; currentIndex: 1 }
            Label { text: "GMYC/ASAP alignment only; tree deduplication needs verified alignment preparation" }
            WorkspaceButton { text: "Count haplotypes"; enabled: !ctl.busy; onClicked: ctl.run_analysis("haplotypes",analysisInput.text,false,"{}") }
            Label { text: ctl.sampling_summary }
        }
        RowLayout {
            Layout.minimumWidth: 0
            visible: doc.settingsOpen
            Label { text: "Distance model" }
            AppComboBox { id: distanceModel; model: ["p", "jc69", "k2p"] }
            Label { text: "Seed" }
            SpinBox { id: seed; from: 1; to: 2147483647; value: 1; editable: true }
            Label { text: "Workers" }
            SpinBox { id: workers; from: 1; to: 256; value: 1; editable: true }
            Label { text: "ASAP simulations per node (maximum)" }
            SpinBox { id: replicates; from: 100; to: 10000000; value: 100000; editable: true }
            Label { text: "Tree index (0 first)" }
            SpinBox { id: treeIndex; from: 0; to: 100000; value: 0; editable: true }
        }
        RowLayout {
            Layout.minimumWidth: 0
            visible: doc.settingsOpen
            TextField { id: cachePath; placeholderText: "Observed distance cache (optional)"; Layout.fillWidth: true }
            TextField { id: mappingPath; placeholderText: "Exact haplotype mapping (optional)"; Layout.fillWidth: true }
        }
            }
        }
        RowLayout {
            visible: doc.tool === "cut" && !doc.settingsOpen
            Layout.fillWidth: true
            Slider { Layout.fillWidth: true; from: 0; to: ctl.root_height; value: ctl.threshold; onMoved: ctl.preview_cut(value) }
            Label { text: ctl.threshold.toPrecision(5) + " · " + JSON.parse(ctl.preview_json).length + " groups" }
            WorkspaceButton { text: "Save cut"; enabled: JSON.parse(ctl.preview_json).length > 0; onClicked: ctl.save_cut(name.text.length ? name.text : "Manual time cut") }
            WorkspaceButton { text: "Cancel"; onClicked: { ctl.cancel_cut(); doc.tool = "select" } }
        }
        Label { text: { ctl.workspace_json; const coverage = JSON.parse(ctl.coverage_json(tree.canvas_geometry)); return "Unmatched tree tips: " + (coverage.unmatched_tips || []).join(", ") + " · Specimens absent from display: " + (coverage.absent_specimens || []).join(", ") } elide: Text.ElideRight; Layout.fillWidth: true }
        SplitView {
            Layout.minimumWidth: 0
            Layout.fillWidth: true
            Layout.fillHeight: true
            ScrollView {
                SplitView.minimumWidth: 120
                SplitView.preferredWidth: 170
                SplitView.maximumWidth: 235
                ColumnLayout {
                    width: parent.width
                    Repeater {
                        model: doc.workspace.hypotheses || []
                        delegate: ColumnLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            RowLayout {
                                CheckBox { checked: (doc.workspace.view.visible || []).includes(modelData.id)
                                    onClicked: ctl.action("visible",JSON.stringify({ids:checked ? (doc.workspace.view.visible || []).concat([modelData.id]) : (doc.workspace.view.visible || []).filter(id => id !== modelData.id)})) }
                                WorkspaceButton { text: modelData.name; highlighted: ctl.active_hypothesis === modelData.id; onClicked: ctl.activate(modelData.id); Layout.fillWidth: true }
                            }
                            RowLayout {
                                AppIconButton { iconName: "arrow-left"; description: "Move hypothesis left"; enabled: (doc.workspace.view.visible || []).includes(modelData.id); onClicked: ctl.action("move_visible",JSON.stringify({id:modelData.id,direction:-1})) }
                                AppIconButton { iconName: "arrow-right"; description: "Move hypothesis right"; enabled: (doc.workspace.view.visible || []).includes(modelData.id); onClicked: ctl.action("move_visible",JSON.stringify({id:modelData.id,direction:1})) }
                            }
                            Label { text: modelData.origin + (modelData.provenance.method.length ? " · " + modelData.provenance.method : ""); elide: Text.ElideRight; Layout.maximumWidth: 220 }
                            Label { text: modelData.groups.length + " groups · " + modelData.unassigned.length + " unassigned" }
                        }
                    }
                }
            }
            Item {
                id: center
                SplitView.fillWidth: true
                TreeCanvas {
                    id: treeCanvas
                    anchors { left: parent.left; top: parent.top; bottom: parent.bottom; right: barArea.left }
                    controller: tree
                    annotationEnabled: false
                    visible: ctl.display_path.length > 0 && !doc.showGroups
                }
                Item {
                    id: cutHandle
                    visible: doc.tool === "cut" && ctl.can_cut && !doc.showGroups
                    readonly property var geometry: JSON.parse(tree.canvas_geometry)
                    x: geometry.root ? geometry.root[0] + (geometry.tips_x - geometry.root[0]) * (1 - ctl.threshold / ctl.root_height) - 7 : 0
                    width: 14; height: treeCanvas.height
                    Rectangle { anchors.horizontalCenter: parent.horizontalCenter; width: 2; height: parent.height; color: "#e41a1c" }
                    Rectangle { anchors.top: parent.top; width: 14; height: 16; radius: 3; color: "#e41a1c" }
                    MouseArea {
                        anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                        onPressed: event => ctl.preview_position(tree.canvas_geometry,mapToItem(treeCanvas,event.x,event.y).x)
                        onPositionChanged: event => { if (pressed) ctl.preview_position(tree.canvas_geometry,mapToItem(treeCanvas,event.x,event.y).x) }
                    }
                }
                ScrollView {
                    anchors.fill: parent
                    visible: doc.showGroups || !ctl.display_path.length
                    contentWidth: availableWidth
                    ColumnLayout {
                        width: parent.width
                        Label { text: "Sequence groups"; font.bold: true }
                        Label { text: "Select sequences, then choose Group selection to define a species."; wrapMode: Text.Wrap; Layout.fillWidth: true }
                        Repeater {
                            model: (doc.workspace.hypotheses || []).filter(h => (doc.workspace.view.visible || []).includes(h.id) || h.id === ctl.active_hypothesis)
                            delegate: ColumnLayout {
                                required property var modelData
                                Layout.fillWidth: true
                                Label { text: modelData.name; font.bold: true }
                                Repeater {
                                    model: modelData.groups
                                    delegate: ColumnLayout {
                                        required property var modelData
                                        Layout.fillWidth: true
                                        Label { text: modelData.label + " · " + modelData.members.length + " sequences"; font.bold: true }
                                        Label { text: modelData.members.join(", "); wrapMode: Text.Wrap; Layout.fillWidth: true }
                                    }
                                }
                                Label { text: "Unassigned: " + modelData.unassigned.join(", "); wrapMode: Text.Wrap; Layout.fillWidth: true }
                            }
                        }
                        Repeater {
                            model: doc.workspace.specimens || []
                            delegate: CheckBox {
                                required property var modelData
                                text: modelData.label
                                checked: doc.selection.includes(modelData.id)
                                onClicked: ctl.highlight_taxa(JSON.stringify(checked ? doc.selection.concat([modelData.id]) : doc.selection.filter(id => id !== modelData.id)))
                            }
                        }
                    }
                }
                Item {
                    id: barArea
                    visible: !doc.showGroups && ctl.display_path.length > 0
                    anchors { right: parent.right; top: parent.top; bottom: parent.bottom }
                    readonly property real naturalWidth: (doc.workspace.view ? doc.workspace.view.visible.length : 0) * 36 + 36
                    width: Math.min(naturalWidth, parent.width * 0.45)
                    clip: true
                    readonly property var segments: {
                        ctl.workspace_json; ctl.preview_json
                        return JSON.parse(ctl.bars_json(tree.canvas_geometry,0,height))
                    }
                    Repeater { model: barArea.segments
                        delegate: Rectangle {
                            required property var modelData
                            x: modelData.x * barArea.width / barArea.naturalWidth; y: modelData.y; width: modelData.width * barArea.width / barArea.naturalWidth; height: modelData.height
                            color: modelData.color.length ? modelData.color : "#377eb8"
                            border.color: "#ffffff"; border.width: 0.5
                            ToolTip.visible: segmentMouse.containsMouse; ToolTip.text: modelData.label
                            MouseArea { id: segmentMouse; anchors.fill: parent; hoverEnabled: true; onClicked: event => ctl.select_group(modelData.hypothesis,modelData.group,!!(event.modifiers & Qt.ControlModifier)) }
                        }
                    }
                }
            }
            ScrollView {
                visible: doc.width > 1000 || doc.detailsOpen
                SplitView.preferredWidth: 290
                ColumnLayout {
                    width: parent.width
                    Label { text: "Groups and exact membership"; font.bold: true }
                    Repeater {
                        model: (doc.workspace.hypotheses || []).filter(h => h.id === ctl.active_hypothesis)
                        delegate: ColumnLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            Label { text: "Unassigned: " + modelData.unassigned.join(", "); wrapMode: Text.Wrap; Layout.maximumWidth: 260 }
                            Repeater { model: modelData.groups
                                delegate: ColumnLayout {
                                    required property var modelData
                                    Layout.fillWidth: true
                                    CheckBox { text: "Include in merge"; checked: doc.mergeGroups.includes(modelData.id); onClicked: doc.mergeGroups = checked ? doc.mergeGroups.concat([modelData.id]) : doc.mergeGroups.filter(id => id !== modelData.id) }
                                    WorkspaceButton { text: modelData.label + " (" + modelData.members.length + ")"; onClicked: ctl.select_group(ctl.active_hypothesis,modelData.id,false) }
                                    Label { text: modelData.members.join(", "); wrapMode: Text.Wrap; Layout.maximumWidth: 260 }
                                    RowLayout {
                                        WorkspaceButton { text: "Reassign here"; enabled: doc.selection.length > 0; onClicked: ctl.action("reassign",JSON.stringify({group:modelData.id})) }
                                        WorkspaceButton { text: "Rename"; onClicked: ctl.action("rename_group",JSON.stringify({group:modelData.id,label:name.text})) }
                                    }
                                }
                            }
                        }
                    }
                    ColumnLayout {
                        id: fitInspector
                        property var fit: { ctl.workspace_json; ctl.active_hypothesis; return JSON.parse(ctl.analysis_json()) }
                        property var best: fit.profile ? fit.profile[fit.best] : ({})
                        visible: fit.profile !== undefined
                        Label { text: "Fitted GMYC " + (fitInspector.fit.method || ""); font.bold: true }
                        WorkspaceButton { text: "Export fit report"; onClicked: ctl.export_analysis(doc.path + ".gmyc.json") }
                        Label { text: "Threshold ages: " + (fitInspector.best.thresholds || []).join(", "); wrapMode: Text.Wrap; Layout.maximumWidth: 260 }
                        Label { text: "Log likelihood: " + fitInspector.best.log_likelihood + " · LR: " + fitInspector.fit.likelihood_ratio + " · p: " + fitInspector.fit.lr_p_value; wrapMode: Text.Wrap; Layout.maximumWidth: 260 }
                        Label { text: "log λ diversification: " + fitInspector.best.log_lambda_div + " · coalescent: " + fitInspector.best.log_lambda_coal + " · exponents: " + fitInspector.best.p_div + ", " + fitInspector.best.p_coal; wrapMode: Text.Wrap; Layout.maximumWidth: 260 }
                        Label { text: (fitInspector.best.diagnostics || []).join("\n"); wrapMode: Text.Wrap; Layout.maximumWidth: 260 }
                        Label { text: fitInspector.fit.support_convention || ""; wrapMode: Text.Wrap; Layout.maximumWidth: 260 }
                        Label { text: "Likelihood profile (age: log likelihood)"; font.bold: true }
                        Repeater { model: fitInspector.fit.profile || []
                            delegate: Label {
                                required property var modelData
                                text: (modelData.thresholds || []).join(", ") + ": " + modelData.log_likelihood.toFixed(6)
                                wrapMode: Text.Wrap; Layout.maximumWidth: 260
                            }
                        }
                    }
                    Label { text: "Observed distances"; font.bold: true }
                    WorkspaceButton { text: "Export summaries CSV"; enabled: (JSON.parse(ctl.statistics_json).groups || []).length > 0; onClicked: ctl.export_statistics(doc.path + ".statistics.csv") }
                    Repeater { model: JSON.parse(ctl.statistics_json).groups || []
                        delegate: Label {
                            required property var modelData
                            text: modelData.group + ": n=" + modelData.specimens + " · mean=" + (modelData.mean_within === null ? "unavailable" : modelData.mean_within) + " · max=" + modelData.max_within + " · min between=" + modelData.min_between + " · nearest=" + (modelData.nearest_groups || []).join(", ") + " · gap=" + (modelData.barcode_gap === null ? "unavailable" : modelData.barcode_gap) + " · observed=" + modelData.observed_within + "/" + modelData.observed_between + " · missing=" + modelData.missing_within + "/" + modelData.missing_between
                            wrapMode: Text.Wrap; Layout.maximumWidth: 260
                        }
                    }
                    Label { text: "Specimens"; font.bold: true }
                    Repeater { model: doc.workspace.specimens || []
                        delegate: CheckBox {
                            required property var modelData
                            text: modelData.label; checked: doc.selection.includes(modelData.id)
                            onClicked: ctl.highlight_taxa(JSON.stringify(checked ? doc.selection.concat([modelData.id]) : doc.selection.filter(id => id !== modelData.id)))
                        }
                    }
                }
            }
        }
    }
    AppDialog {
        id: detailsDialog
        title: "Analysis error details"
        implicitWidth: 760
        implicitHeight: 480
        standardButtons: Dialog.Close
        initialFocusItem: detailsText
        contentItem: ColumnLayout {
            Label { text: ctl.message; wrapMode: Text.Wrap; Layout.fillWidth: true }
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                TextArea { id: detailsText; text: ctl.message_details; readOnly: true; selectByMouse: true; wrapMode: TextEdit.Wrap; font.family: "monospace"; Accessible.name: "Full error output" }
            }
            AppButton { text: "Copy"; onClicked: { detailsText.selectAll(); detailsText.copy(); detailsText.deselect() } }
        }
    }
    Shortcut { sequence: "Escape"; onActivated: { ctl.cancel_cut(); doc.tool = "select" } }
    Shortcut { sequence: "Ctrl+G"; onActivated: ctl.action("group",JSON.stringify({label:name.text})) }
}
