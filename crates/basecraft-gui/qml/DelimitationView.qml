import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Basecraft

Item {
    id: doc
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
        RowLayout {
            Label { text: "Hypotheses"; font.bold: true }
            TextField { id: name; placeholderText: "Hypothesis or group name"; Layout.preferredWidth: 210 }
            Button { text: "New"; onClicked: ctl.action("create", JSON.stringify({name:name.text})) }
            Button { text: "Derive"; enabled: ctl.active_hypothesis.length > 0; onClicked: ctl.action("derive", JSON.stringify({name:name.text})) }
            Button { text: "Rename"; onClicked: ctl.action("rename",JSON.stringify({name:name.text})) }
            Button { text: "Group selection"; enabled: doc.selection.length > 0; onClicked: ctl.action("group",JSON.stringify({label:name.text})) }
            Button { text: "Merge checked groups"; enabled: doc.mergeGroups.length > 1; onClicked: { ctl.action("merge",JSON.stringify({groups:doc.mergeGroups,label:name.text})); doc.mergeGroups = [] } }
            Button { text: "Unassign"; enabled: doc.selection.length > 0; onClicked: ctl.action("unassign","{}") }
        }
        RowLayout {
            TextField { id: treePath; placeholderText: "Display tree path"; Layout.fillWidth: true; text: ctl.display_path }
            CheckBox { id: rooted; text: "Explicitly rooted time tree" }
            Button { text: "Attach tree"; onClicked: ctl.attach_tree(treePath.text,rooted.checked) }
            Button { text: "Select"; checkable: true; checked: doc.tool === "select"; onClicked: { doc.tool = "select"; ctl.cancel_cut() } }
            Button { text: "Cut"; checkable: true; checked: doc.tool === "cut"; enabled: ctl.can_cut; onClicked: doc.tool = "cut" }
            Button { text: "SVG"; onClicked: ctl.export_svg(doc.path + ".svg") }
        }
        RowLayout {
            visible: doc.tool === "cut"
            Label { text: "Manual time cut (age ≤ threshold)" }
            Slider { id: cutSlider; Layout.fillWidth: true; from: 0; to: ctl.root_height; value: ctl.threshold; onMoved: ctl.preview_cut(value) }
            TextField { id: cutValue; text: ctl.threshold.toPrecision(6); Layout.preferredWidth: 110; validator: DoubleValidator { bottom: 0; top: ctl.root_height } onEditingFinished: ctl.preview_cut(Number(text)) }
            Label { text: JSON.parse(ctl.preview_json).length + " preview groups" }
            Button { text: "Save cut"; enabled: JSON.parse(ctl.preview_json).length > 0; onClicked: ctl.save_cut(name.text.length ? name.text : "Manual time cut") }
            Button { text: "Cancel"; onClicked: { ctl.cancel_cut(); doc.tool = "select" } }
        }
        RowLayout {
            TextField { id: analysisInput; placeholderText: "Immutable analysis input (tree or aligned FASTA)"; Layout.fillWidth: true; text: ctl.display_path }
            ComboBox { id: method; model: ["gmyc_single", "gmyc_multiple", "ptp", "mptp", "asap", "relative_time", "haplotypes", "group_statistics"] }
            Button { text: "Run"; enabled: !ctl.busy; onClicked: ctl.run_analysis(method.currentText,analysisInput.text,rooted.checked,JSON.stringify({cache:cachePath.text,mapping:mappingPath.text})) }
            Button { text: "Cancel job"; enabled: ctl.busy; onClicked: ctl.cancel_job() }
        }
        RowLayout {
            TextField { id: cachePath; placeholderText: "Observed distance cache (optional)"; Layout.fillWidth: true }
            TextField { id: mappingPath; placeholderText: "Exact haplotype mapping (optional)"; Layout.fillWidth: true }
        }
        Label { text: { ctl.workspace_json; const coverage = JSON.parse(ctl.coverage_json(tree.canvas_geometry)); return "Unmatched tree tips: " + (coverage.unmatched_tips || []).join(", ") + " · Specimens absent from display: " + (coverage.absent_specimens || []).join(", ") } elide: Text.ElideRight; Layout.fillWidth: true }
        SplitView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            ScrollView {
                SplitView.preferredWidth: 235
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
                                Button { text: modelData.name; highlighted: ctl.active_hypothesis === modelData.id; onClicked: ctl.activate(modelData.id); Layout.fillWidth: true }
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
                    visible: ctl.display_path.length > 0
                }
                Item {
                    id: cutHandle
                    visible: doc.tool === "cut" && ctl.can_cut
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
                Label { anchors.centerIn: parent; visible: !treeCanvas.visible; text: "Attach a tree or use the specimen table" }
                Item {
                    id: barArea
                    anchors { right: parent.right; top: parent.top; bottom: parent.bottom }
                    width: (doc.workspace.view ? doc.workspace.view.visible.length : 0) * 36 + 36
                    clip: true
                    readonly property var segments: {
                        ctl.workspace_json; ctl.preview_json
                        return JSON.parse(ctl.bars_json(tree.canvas_geometry,0,height))
                    }
                    Repeater { model: barArea.segments
                        delegate: Rectangle {
                            required property var modelData
                            x: modelData.x; y: modelData.y; width: modelData.width; height: modelData.height
                            color: modelData.color.length ? modelData.color : "#377eb8"
                            border.color: "#ffffff"; border.width: 0.5
                            ToolTip.visible: segmentMouse.containsMouse; ToolTip.text: modelData.label
                            MouseArea { id: segmentMouse; anchors.fill: parent; hoverEnabled: true; onClicked: event => ctl.select_group(modelData.hypothesis,modelData.group,!!(event.modifiers & Qt.ControlModifier)) }
                        }
                    }
                }
            }
            ScrollView {
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
                                    Button { text: modelData.label + " (" + modelData.members.length + ")"; onClicked: ctl.select_group(ctl.active_hypothesis,modelData.id,false) }
                                    Label { text: modelData.members.join(", "); wrapMode: Text.Wrap; Layout.maximumWidth: 260 }
                                    RowLayout {
                                        Button { text: "Reassign here"; enabled: doc.selection.length > 0; onClicked: ctl.action("reassign",JSON.stringify({group:modelData.id})) }
                                        Button { text: "Rename"; onClicked: ctl.action("rename_group",JSON.stringify({group:modelData.id,label:name.text})) }
                                    }
                                }
                            }
                        }
                    }
                    Label { text: "Observed distances"; font.bold: true }
                    Repeater { model: JSON.parse(ctl.statistics_json).groups || []
                        delegate: Label {
                            required property var modelData
                            text: modelData.group + ": n=" + modelData.specimens + " · mean=" + (modelData.mean_within === null ? "unavailable" : modelData.mean_within) + " · max=" + modelData.max_within + " · nearest=" + (modelData.nearest_groups || []).join(", ") + " · missing=" + modelData.missing_within + "/" + modelData.missing_between
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
    Shortcut { sequence: "Escape"; onActivated: { ctl.cancel_cut(); doc.tool = "select" } }
    Shortcut { sequence: "Ctrl+G"; onActivated: ctl.action("group",JSON.stringify({label:name.text})) }
}
