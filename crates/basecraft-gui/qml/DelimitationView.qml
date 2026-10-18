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
    readonly property var hypotheses: workspace.hypotheses || []
    readonly property var selection: JSON.parse(ctl.selection_json)
    // Visible columns left to right, with letter codes and the legend.
    readonly property var columnInfo: { ctl.workspace_json; ctl.theme_revision; return JSON.parse(ctl.columns_json(0)) }
    readonly property var activeHypothesis: hypotheses.find(h => h.id === ctl.active_hypothesis) || null
    property string tool: "select"
    property bool detailsOpen: false
    // Tip names of the clade last selected on the tree, for Flip subtree.
    property var treeTaxa: []
    // Edits change only manual hypotheses and edited copies.
    readonly property bool activeEditable: activeHypothesis !== null && !["imported", "inferred"].includes(activeHypothesis.origin)
    property var pendingDisplay: null
    // Created by an analysis request; discarded on close if nothing came of it.
    property bool draft: false
    property bool ribbonInitialized: false
    property bool showGroups: !ctl.display_path.length
    property alias treeController: tree
    readonly property color background: tree.background
    readonly property color foreground: tree.foreground

    function act(name, args) { ctl.action(name, JSON.stringify(args || {})) }
    function nextSpeciesLabel() {
        const used = new Set(((activeHypothesis || {}).groups || []).map(g => g.label))
        let n = ((activeHypothesis || {}).groups || []).length + 1
        while (used.has("Species " + n)) n++
        return "Species " + n
    }
    // Ribbon commands.
    function newHypothesis(source) { newDialog.show(source || "") }
    function groupSelection() { if (selection.length) act("group", {label: nextSpeciesLabel()}) }
    function unassign() { if (selection.length) act("unassign") }
    // The species of the edited hypothesis that is exactly the selection, if any.
    function selectedSpecies() {
        const chosen = new Set(selection)
        return ((activeHypothesis || {}).groups || []).find(g => g.members.length === chosen.size && g.members.every(m => chosen.has(m))) || null
    }
    // Rename the selected species, or make the selection a species with this
    // name; either way the names show beside the bars.
    function nameSpecies(label) {
        if (!activeHypothesis || !selection.length) return
        const existing = selectedSpecies()
        if (existing) act("rename_group", {group: existing.id, label: label})
        else act("group", {label: label})
        const style = (workspace.view.columns || {})[activeHypothesis.id] || {}
        if (!style.names) act("style", {hypothesis: activeHypothesis.id, names: true})
    }
    function askSpeciesName() {
        const existing = selectedSpecies()
        textDialog.ask("Name species", "Species name", existing ? existing.label : nextSpeciesLabel(), value => doc.nameSpecies(value))
    }
    function flipSubtree() { if (treeTaxa.length) act("flip", {tips: treeTaxa}) }
    function setTool(name) {
        if (name !== "cut") ctl.cancel_cut()
        // Start the cut halfway down the tree so the line and its species show at once.
        else if (ctl.can_cut && tool !== "cut") ctl.preview_cut(ctl.threshold > 0 ? ctl.threshold : ctl.root_height / 2)
        tool = name
    }
    function chooseColumns() { columnsDialog.show() }

    function startAnalysis(method, input, rooted, settings, displayMethod) {
        if (ctl.busy) return
        if (method === "manual" || method === "view") {
            if (method === "manual") {
                act("create", {name: "Manual species hypothesis"})
                const created = JSON.parse(ctl.workspace_json).hypotheses || []
                if (created.length) ctl.activate(created[created.length - 1].id)
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
    // Hidden menu items take no space.
    component TreeMenuItem: MenuItem { height: visible ? implicitHeight : 0 }
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
    Component.onCompleted: {
        tree.check_theme()
        ctl.check_theme()
        ctl.open_file(path)
    }
    Connections { target: ctl
        function onShown_treeChanged() { if (ctl.shown_tree.length) tree.open_file(ctl.shown_tree) }
        function onDisplayTaxaSelected(names) { tree.highlight_taxa(names) }
    }
    Connections { target: tree
        function onTaxaSelected(names) {
            doc.treeTaxa = JSON.parse(names)
            ctl.tree_taxa(names)
        }
    }
    Rectangle { anchors.fill: parent; color: doc.background }
    ColumnLayout {
        anchors.fill: parent
        spacing: 4
        AppMessageBar {
            Layout.fillWidth: true
            message: ctl.message
            kind: ctl.message_kind
            details: ctl.message_details
            busy: ctl.busy
            onDetailsRequested: detailsDialog.open()
            onCancelRequested: ctl.cancel_job()
        }
        RowLayout {
            objectName: "thresholdCut"
            visible: doc.tool === "cut"
            Layout.fillWidth: true
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            Label { text: "Manual threshold cut"; font.bold: true }
            Slider { Layout.fillWidth: true; from: 0; to: ctl.root_height; value: ctl.threshold; onMoved: ctl.preview_cut(value); Accessible.name: "Threshold" }
            TextField {
                text: ctl.threshold.toPrecision(5)
                Layout.preferredWidth: 110
                validator: DoubleValidator { bottom: 0; top: ctl.root_height }
                onEditingFinished: ctl.preview_cut(Number(text))
                Accessible.name: "Threshold value"
            }
            Label { text: "Threshold " + Number(ctl.threshold.toPrecision(4)) + " · " + JSON.parse(ctl.preview_json).length + " species" }
            WorkspaceButton {
                text: "Save cut"
                enabled: JSON.parse(ctl.preview_json).length > 0
                onClicked: {
                    ctl.save_cut("Manual threshold cut at " + Number(ctl.threshold.toPrecision(4)))
                    // Saving clears the preview; a failed save keeps the cut open.
                    if (JSON.parse(ctl.preview_json).length === 0) doc.setTool("select")
                }
            }
            WorkspaceButton { text: "Cancel"; onClicked: doc.setTool("select") }
        }
        Label {
            readonly property var coverage: { ctl.workspace_json; return JSON.parse(ctl.coverage_json(tree.canvas_geometry)) }
            visible: !doc.showGroups && ((coverage.unmatched_tips || []).length + (coverage.absent_specimens || []).length) > 0
            text: "Unmatched tree tips: " + (coverage.unmatched_tips || []).join(", ") + " · Specimens absent from display: " + (coverage.absent_specimens || []).join(", ")
            elide: Text.ElideRight
            color: doc.foreground
            Layout.fillWidth: true
            Layout.leftMargin: 8
        }
        SplitView {
            Layout.minimumWidth: 0
            Layout.fillWidth: true
            Layout.fillHeight: true
            Item {
                id: center
                SplitView.fillWidth: true
                readonly property bool treeShown: ctl.display_path.length > 0 && !doc.showGroups
                // One letter per column: its method. Left click edits that
                // hypothesis; right click opens its options.
                Item {
                    id: header
                    visible: center.treeShown
                    anchors { left: parent.left; right: parent.right; top: parent.top }
                    height: visible ? 22 : 0
                    Repeater {
                        model: doc.columnInfo.columns
                        delegate: Item {
                            id: columnHeader
                            required property var modelData
                            required property int index
                            objectName: "columnHeader-" + modelData.id
                            readonly property bool active: modelData.id === ctl.active_hypothesis
                            x: barArea.x + modelData.x + modelData.width / 2 - width / 2
                            width: Math.max(14, modelData.width)
                            height: header.height
                            Label {
                                anchors.centerIn: parent
                                text: columnHeader.modelData.code
                                font.bold: columnHeader.active
                                color: doc.foreground
                            }
                            Rectangle {
                                visible: columnHeader.active
                                anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                                height: 2
                                color: tree.accent
                            }
                            MouseArea {
                                id: headerMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                acceptedButtons: Qt.LeftButton | Qt.RightButton
                                onClicked: event => {
                                    if (event.button === Qt.RightButton) columnMenu.openFor(columnHeader.modelData, columnHeader.index)
                                    else ctl.activate(columnHeader.modelData.id)
                                }
                            }
                            ToolTip.visible: headerMouse.containsMouse
                            ToolTip.text: modelData.name + "\n" + modelData.method + ". Right-click for options."
                            Accessible.role: Accessible.ColumnHeader
                            Accessible.name: modelData.method + ": " + modelData.name
                        }
                    }
                }
                TreeCanvas {
                    id: treeCanvas
                    anchors { left: parent.left; top: header.bottom; bottom: legend.top; right: barArea.left }
                    controller: tree
                    annotationEnabled: false
                    visible: center.treeShown
                    onContextMenu: info => treeMenu.openFor(info)
                }
                Item {
                    id: cutHandle
                    visible: doc.tool === "cut" && ctl.can_cut && center.treeShown
                    readonly property var geometry: JSON.parse(tree.canvas_geometry)
                    x: geometry.root ? geometry.root[0] + (geometry.tips_x - geometry.root[0]) * (1 - ctl.threshold / ctl.root_height) - 7 : 0
                    y: treeCanvas.y
                    width: 14; height: treeCanvas.height
                    Rectangle { anchors.horizontalCenter: parent.horizontalCenter; width: 2; height: parent.height; color: tree.error_color }
                    Rectangle { anchors.top: parent.top; width: 14; height: 16; radius: 3; color: tree.error_color }
                    MouseArea {
                        anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                        onPressed: event => ctl.preview_position(tree.canvas_geometry,mapToItem(treeCanvas,event.x,event.y).x)
                        onPositionChanged: event => { if (pressed) ctl.preview_position(tree.canvas_geometry,mapToItem(treeCanvas,event.x,event.y).x) }
                    }
                }
                Item {
                    id: barArea
                    visible: center.treeShown
                    anchors { right: nameArea.left; top: header.bottom; bottom: legend.top }
                    readonly property var lastColumn: doc.columnInfo.columns.length ? doc.columnInfo.columns[doc.columnInfo.columns.length - 1] : null
                    width: (lastColumn ? lastColumn.x + lastColumn.width : 0) + 12
                    clip: true
                    readonly property var segments: {
                        ctl.workspace_json; ctl.preview_json; ctl.theme_revision
                        return JSON.parse(ctl.bars_json(tree.canvas_geometry,0,height))
                    }
                    Repeater { model: barArea.segments
                        delegate: Rectangle {
                            required property var modelData
                            x: modelData.x; y: modelData.y; width: modelData.width; height: modelData.height
                            color: modelData.color
                            border.color: doc.background; border.width: 0.5
                            ToolTip.visible: segmentMouse.containsMouse; ToolTip.text: modelData.label
                            MouseArea {
                                id: segmentMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                acceptedButtons: Qt.LeftButton | Qt.RightButton
                                onClicked: event => {
                                    if (event.button === Qt.RightButton) speciesMenu.openFor(parent.modelData)
                                    else ctl.select_group(parent.modelData.hypothesis, parent.modelData.group, !!(event.modifiers & Qt.ControlModifier))
                                }
                            }
                        }
                    }
                }
                // Species names for the columns that show them, after the bars.
                Row {
                    id: nameArea
                    objectName: "speciesNames"
                    visible: center.treeShown
                    anchors { right: parent.right; top: header.bottom; bottom: legend.top }
                    spacing: 8
                    Repeater {
                        model: doc.columnInfo.columns.filter(c => c.names)
                        delegate: Item {
                            id: nameColumn
                            required property var modelData
                            // One name per run of touching segments, as in the SVG export.
                            readonly property var runs: {
                                const column = barArea.segments.filter(s => s.hypothesis === nameColumn.modelData.id).sort((a, b) => a.y - b.y)
                                const out = []
                                for (const s of column) {
                                    const last = out[out.length - 1]
                                    if (last && last.group === s.group && s.y <= last.bottom + 1) last.bottom = s.y + s.height
                                    else out.push({group: s.group, label: s.label, top: s.y, bottom: s.y + s.height})
                                }
                                return out
                            }
                            height: nameArea.height
                            width: Math.max(0, ...nameColumn.modelData.groups.map(g => nameMetrics.advanceWidth(g.label))) + 4
                            Repeater {
                                model: nameColumn.runs
                                delegate: Label {
                                    required property var modelData
                                    y: (modelData.top + modelData.bottom) / 2 - height / 2
                                    text: modelData.label
                                    color: doc.foreground
                                }
                            }
                        }
                    }
                    FontMetrics { id: nameMetrics }
                }
                Label {
                    id: legend
                    objectName: "methodLegend"
                    visible: center.treeShown && doc.columnInfo.legend.length > 0
                    anchors { right: parent.right; bottom: parent.bottom; rightMargin: 8 }
                    height: visible ? implicitHeight + 6 : 0
                    verticalAlignment: Text.AlignVCenter
                    color: doc.foreground
                    text: doc.columnInfo.legend.map(entry => entry.code + " " + entry.method).join("   ")
                }
                ScrollView {
                    anchors.fill: parent
                    visible: !center.treeShown
                    contentWidth: availableWidth
                    ColumnLayout {
                        width: parent.width
                        TapHandler { acceptedButtons: Qt.RightButton; onTapped: treeMenu.openFor({}) }
                        Label { text: "Species hypotheses"; font.bold: true; color: doc.foreground }
                        Label { text: "Tick sequences, then right-click to group or name them as a species."; wrapMode: Text.Wrap; Layout.fillWidth: true; color: doc.foreground }
                        Repeater {
                            model: doc.hypotheses.filter(h => (doc.workspace.view.visible || []).includes(h.id) || h.id === ctl.active_hypothesis)
                            delegate: ColumnLayout {
                                required property var modelData
                                Layout.fillWidth: true
                                Label { text: modelData.name; font.bold: true; color: doc.foreground }
                                Repeater {
                                    model: modelData.groups
                                    delegate: ColumnLayout {
                                        required property var modelData
                                        Layout.fillWidth: true
                                        Label { text: modelData.label + " · " + modelData.members.length + " sequences"; font.bold: true; color: doc.foreground }
                                        Label { text: modelData.members.join(", "); wrapMode: Text.Wrap; Layout.fillWidth: true; color: doc.foreground }
                                    }
                                }
                                Label { text: "Unassigned: " + modelData.unassigned.join(", "); wrapMode: Text.Wrap; Layout.fillWidth: true; color: doc.foreground }
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
            }
            ScrollView {
                visible: doc.detailsOpen
                SplitView.preferredWidth: 290
                ColumnLayout {
                    width: parent.width
                    Label { text: doc.activeHypothesis ? "Species in " + doc.activeHypothesis.name : "Click a column letter to choose a hypothesis"; font.bold: true; wrapMode: Text.Wrap; Layout.maximumWidth: 260 }
                    Repeater {
                        model: doc.activeHypothesis ? [doc.activeHypothesis] : []
                        delegate: ColumnLayout {
                            id: activeBlock
                            required property var modelData
                            Layout.fillWidth: true
                            Label { text: "Unassigned: " + activeBlock.modelData.unassigned.join(", "); wrapMode: Text.Wrap; Layout.maximumWidth: 260 }
                            Repeater { model: activeBlock.modelData.groups
                                delegate: ColumnLayout {
                                    id: groupBlock
                                    required property var modelData
                                    Layout.fillWidth: true
                                    WorkspaceButton {
                                        text: groupBlock.modelData.label + " (" + groupBlock.modelData.members.length + ")"
                                        onClicked: ctl.select_group(ctl.active_hypothesis, groupBlock.modelData.id, false)
                                        TapHandler {
                                            acceptedButtons: Qt.RightButton
                                            onTapped: speciesMenu.openFor({hypothesis: activeBlock.modelData.id, group: groupBlock.modelData.id, label: groupBlock.modelData.label})
                                        }
                                    }
                                    Label { text: groupBlock.modelData.members.join(", "); wrapMode: Text.Wrap; Layout.maximumWidth: 260 }
                                    WorkspaceButton { text: "Reassign here"; enabled: doc.selection.length > 0; onClicked: doc.act("reassign", {group: groupBlock.modelData.id}) }
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

    Menu {
        id: columnMenu
        objectName: "columnMenu"
        property var column: ({})
        property int position: 0
        function openFor(column, position) {
            columnMenu.column = column
            columnMenu.position = position
            popup()
        }
        MenuItem { text: columnMenu.column.name || ""; enabled: false }
        MenuSeparator {}
        MenuItem { text: "Edit this hypothesis"; checkable: true; checked: columnMenu.column.id === ctl.active_hypothesis; onTriggered: ctl.activate(columnMenu.column.id) }
        MenuItem { text: "Move left"; enabled: columnMenu.position > 0; onTriggered: doc.act("move_visible", {id: columnMenu.column.id, direction: -1}) }
        MenuItem { text: "Move right"; enabled: columnMenu.position < doc.columnInfo.columns.length - 1; onTriggered: doc.act("move_visible", {id: columnMenu.column.id, direction: 1}) }
        MenuSeparator {}
        MenuItem { text: "Rename…"; onTriggered: textDialog.ask("Rename hypothesis", "Name", columnMenu.column.name, value => doc.act("rename", {hypothesis: columnMenu.column.id, name: value})) }
        MenuItem { text: "Bar width…"; onTriggered: widthDialog.show(columnMenu.column) }
        MenuItem { text: "Colours…"; onTriggered: colorDialog.showFor(columnMenu.column, "") }
        MenuItem { text: "Show species names"; checkable: true; checked: columnMenu.column.names === true; onTriggered: doc.act("style", {hypothesis: columnMenu.column.id, names: checked}) }
        MenuItem { text: "Edit a copy…"; onTriggered: doc.newHypothesis(columnMenu.column.id) }
        MenuSeparator {}
        MenuItem { text: "Hide column"; onTriggered: doc.act("hide", {hypothesis: columnMenu.column.id}) }
    }
    Menu {
        id: speciesMenu
        property var segment: ({})
        readonly property var column: doc.columnInfo.columns.find(c => c.id === segment.hypothesis) || ({})
        function openFor(segment) {
            speciesMenu.segment = segment
            popup()
        }
        MenuItem { text: speciesMenu.segment.label || ""; enabled: false }
        MenuSeparator {}
        MenuItem { text: "Select species"; onTriggered: ctl.select_group(speciesMenu.segment.hypothesis, speciesMenu.segment.group, false) }
        MenuItem {
            text: speciesMenu.column.editable ? "Rename species…" : "Rename species… (make an edited copy first)"
            enabled: speciesMenu.column.editable === true
            onTriggered: textDialog.ask("Rename species", "Name", speciesMenu.segment.label, value => doc.act("rename_group", {hypothesis: speciesMenu.segment.hypothesis, group: speciesMenu.segment.group, label: value}))
        }
        MenuItem { text: "Colour…"; enabled: speciesMenu.column.id !== undefined; onTriggered: colorDialog.showFor(speciesMenu.column, speciesMenu.segment.group) }
    }

    // Right-click on the tree, or in the text view: edit the hypothesis being
    // edited with the selected sequences.
    Menu {
        id: treeMenu
        objectName: "speciesTreeMenu"
        property var info: ({})
        function openFor(info) {
            treeMenu.info = info || {}
            popup()
        }
        readonly property bool canEdit: doc.activeEditable && doc.selection.length > 0 && !ctl.busy
        TreeMenuItem {
            text: doc.activeHypothesis ? (doc.activeEditable ? "Editing " + doc.activeHypothesis.name : doc.activeHypothesis.name + ": make an edited copy to change it") : "Click a column letter to choose a hypothesis"
            enabled: false
        }
        MenuSeparator {}
        TreeMenuItem { text: "Group selection"; objectName: "groupSelection"; enabled: treeMenu.canEdit; onTriggered: doc.groupSelection() }
        TreeMenuItem { text: doc.selectedSpecies() ? "Rename species…" : "Name species…"; objectName: "nameSpecies"; enabled: treeMenu.canEdit; onTriggered: doc.askSpeciesName() }
        TreeMenuItem { text: "Unassign"; objectName: "unassign"; enabled: treeMenu.canEdit; onTriggered: doc.unassign() }
        MenuSeparator { visible: treeMenu.info.node === true; height: visible ? implicitHeight : 0 }
        TreeMenuItem { text: "Flip subtree"; objectName: "flipSubtree"; visible: treeMenu.info.node === true; enabled: !ctl.busy; onTriggered: doc.flipSubtree() }
    }
    AppContentDialog {
        id: newDialog
        objectName: "newHypothesisDialog"
        title: "New species hypothesis"
        standardButtons: Dialog.Ok | Dialog.Cancel
        initialFocusItem: newName
        function show(source) {
            newName.text = "Species hypothesis " + (doc.hypotheses.length + 1)
            const index = doc.hypotheses.findIndex(h => h.id === source)
            startFrom.currentIndex = index + 1
            open()
        }
        onOpened: standardButton(Dialog.Ok).enabled = Qt.binding(() => newName.text.trim().length > 0)
        onAccepted: {
            const args = {name: newName.text.trim()}
            if (startFrom.currentIndex > 0) args.source = doc.hypotheses[startFrom.currentIndex - 1].id
            doc.act("create", args)
        }
        body: ColumnLayout {
            Label { id: newNameLabel; text: "Name" }
            TextField { id: newName; objectName: "newHypothesisName"; Layout.fillWidth: true; Accessible.name: newNameLabel.text }
            Label { id: startFromLabel; text: "Start from" }
            AppComboBox {
                id: startFrom
                objectName: "newHypothesisSource"
                Layout.fillWidth: true
                model: ["Empty: every specimen unassigned"].concat(doc.hypotheses.map(h => "Copy of " + h.name))
                Accessible.name: startFromLabel.text
            }
            Label {
                text: "A copy can be edited even when its source is an analysis result."
                wrapMode: Text.Wrap
                Layout.fillWidth: true
                opacity: 0.75
            }
        }
    }
    AppContentDialog {
        id: textDialog
        property var done: null
        standardButtons: Dialog.Ok | Dialog.Cancel
        initialFocusItem: textValue
        function ask(heading, label, value, callback) {
            title = heading
            textLabel.text = label
            textValue.text = value
            done = callback
            open()
        }
        onOpened: { standardButton(Dialog.Ok).enabled = Qt.binding(() => textValue.text.trim().length > 0); textValue.selectAll() }
        onAccepted: if (done) done(textValue.text.trim())
        body: ColumnLayout {
            Label { id: textLabel }
            TextField { id: textValue; objectName: "renameValue"; Layout.fillWidth: true; Accessible.name: textLabel.text }
        }
    }
    AppContentDialog {
        id: widthDialog
        property string target: ""
        title: "Bar width"
        standardButtons: Dialog.Ok | Dialog.Cancel
        initialFocusItem: widthValue
        function show(column) {
            target = column.id
            widthValue.value = Math.round(column.width)
            open()
        }
        onAccepted: doc.act("style", {hypothesis: target, width: widthValue.value})
        body: RowLayout {
            Label { id: widthLabel; text: "Width in pixels (5 to 200)" }
            AppSpinBox { id: widthValue; objectName: "barWidth"; from: 5; to: 200; editable: true; Accessible.name: widthLabel.text }
        }
    }
    SpeciesColorDialog {
        id: colorDialog
        property string target: ""
        function showFor(column, group) {
            target = column.id
            show(column.name, column.groups)
            const index = column.groups.findIndex(g => g.id === group)
            if (index >= 0) current = index
        }
        onColorsChosen: colors => { if (Object.keys(colors).length) doc.act("style", {hypothesis: target, colors: colors}) }
    }
    AppContentDialog {
        id: columnsDialog
        title: "Columns shown"
        standardButtons: Dialog.Ok | Dialog.Cancel
        property var checked: []
        function show() {
            checked = (doc.workspace.view.visible || []).slice()
            open()
        }
        onAccepted: {
            const current = (doc.workspace.view.visible || []).filter(id => checked.includes(id))
            const added = doc.hypotheses.map(h => h.id).filter(id => checked.includes(id) && !current.includes(id))
            doc.act("visible", {ids: current.concat(added)})
        }
        body: ColumnLayout {
            Label { text: "Each checked hypothesis gets a bar column. Right-click a column letter to move it."; wrapMode: Text.Wrap; Layout.fillWidth: true }
            Repeater {
                model: doc.hypotheses
                delegate: CheckBox {
                    required property var modelData
                    text: modelData.name
                    checked: columnsDialog.checked.includes(modelData.id)
                    onToggled: columnsDialog.checked = checked ? columnsDialog.checked.concat([modelData.id]) : columnsDialog.checked.filter(id => id !== modelData.id)
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
    Shortcut { sequence: "Escape"; onActivated: doc.setTool("select") }
    Shortcut { sequence: "Ctrl+G"; onActivated: if (doc.activeEditable) doc.groupSelection() }
}
