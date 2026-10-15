import QtQuick
import QtQuick.Layouts
import QtQuick.Controls.Basic
import QtQuick.Dialogs
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

    required property var fileUrls
    required property string path
    required property int index
    property alias controller: ctl
    property alias view: view
    readonly property string hoverStatus: visible ? traceMouse.siteText : ""
    readonly property bool dirty: ctl.dirty
    readonly property string kind: "trace"
    property string specimenId: ctl.proposed_name
    signal copyText(string text)
    signal redraft(var paths, string reference, var target, int cutoff)
    property string reviewReference: ""
    property bool sourceInitialized: false
    function hoverAt(x, y) { traceMouse.siteText = ctl.hover(x, y) }
    function issue(forward) { ctl.next_issue(forward); viewport.contentY = 0; view.forceActiveFocus() }
    function accept() { if (ctl.review_set("accept", "")) issue(true) }
    function saveReview() {
        saveReviewDialog.currentFolder = doc.activeFolder()
        saveReviewDialog.selectedFile = fileUrls.local_file_url(ctl.export_path.replace(/_consensus_from_basecraft\.fasta$/, ".sanger-review.json"))
        saveReviewDialog.open()
    }
    function alignmentContext() { alignmentDialog.open() }
    function draftReads() { additionalDialog.open() }
    function openDraft() { draftDialog.open() }
    function openReview() { openReviewDialog.open() }
    function activeFolder() { return fileUrls.parent_folder_url(ctl.export_path) }
    function exportConsensus() { beginExport("fasta") }
    function copyConsensus() { beginExport("clipboard") }
    function beginExport(target) { exportName.target = target; exportId.text = doc.specimenId; exportName.open() }
    function confirmExportName() {
        const fasta = ctl.reviewed_fasta(exportId.text)
        if (!fasta.length) return
        doc.specimenId = exportId.text.trim()
        exportName.close()
        if (exportName.target === "clipboard") { doc.copyText(fasta); view.forceActiveFocus() }
        else {
            exportConsensusDialog.currentFolder = doc.activeFolder()
            exportConsensusDialog.selectedFile = fileUrls.local_file_url(ctl.export_path)
            exportConsensusDialog.open()
        }
    }
    AppContentDialog {
        id: exportName
        initialFocusItem: exportId
        property string target: "fasta"
        title: target === "clipboard" ? "Copy trace consensus" : "Export trace consensus"
        modal: true
        onOpened: { ctl.message = ""; exportId.forceActiveFocus(); exportId.selectAll() }
        body: ColumnLayout {
            spacing: 8
            Label { id: exportIdLabel; text: "Sequence ID" }
            TextField { id: exportId; objectName: "exportId"; Accessible.labelledBy: exportIdLabel; Accessible.name: exportIdLabel.text; Layout.fillWidth: true; onAccepted: doc.confirmExportName() }
            AppErrorLabel { text: ctl.message; visible: text.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap; color: ctl.accent }
            RowLayout {
                Item { Layout.fillWidth: true }
                AppButton { text: "Cancel"; onClicked: exportName.close() }
                AppButton { text: exportName.target === "clipboard" ? "Copy" : "Choose FASTA file…"; enabled: exportId.text.trim().length > 0; onClicked: doc.confirmExportName() }
            }
        }
    }
    Connections { target: ctl; function onProposed_nameChanged() { doc.specimenId = ctl.proposed_name } }
    Shortcut { sequence: "Ctrl+Alt+O"; enabled: doc.visible && !ctl.dirty; onActivated: doc.openReview() }
    Shortcut { sequence: "Ctrl+Shift+D"; enabled: doc.visible && !ctl.dirty; onActivated: doc.openDraft() }
    Shortcut { sequence: "Ctrl+="; enabled: doc.visible; onActivated: ctl.zoom_in() }
    Shortcut { sequence: "Ctrl++"; enabled: doc.visible; onActivated: ctl.zoom_in() }
    Shortcut { sequence: "Ctrl+-"; enabled: doc.visible; onActivated: ctl.zoom_out() }
    Shortcut { sequence: "Ctrl+0"; enabled: doc.visible; onActivated: ctl.fit() }
    Shortcut { sequence: "Alt+Left"; enabled: doc.visible && ctl.review_mode; onActivated: doc.issue(false) }
    Shortcut { sequence: "Alt+Right"; enabled: doc.visible && ctl.review_mode; onActivated: doc.issue(true) }
    Shortcut { sequence: "Alt+Return"; enabled: doc.visible && ctl.review_mode; onActivated: doc.accept() }
    Shortcut { sequence: "Ctrl+Shift+E"; enabled: doc.visible && ctl.review_mode && ctl.unresolved === 0; onActivated: doc.exportConsensus() }
    Shortcut { sequence: "Ctrl+Shift+C"; enabled: doc.visible && ctl.review_mode && ctl.unresolved === 0; onActivated: doc.copyConsensus() }
    Shortcut { sequence: "Ctrl+Shift+A"; enabled: doc.visible && ctl.review_mode && ctl.unresolved === 0 && doc.returnAlignment !== null; onActivated: doc.addToAlignment() }
    Shortcut { sequence: "Ctrl+Alt+A"; enabled: doc.visible && ctl.review_mode && doc.returnAlignment === null; onActivated: doc.alignmentContext() }
    Shortcut { sequence: "Ctrl+Alt+D"; enabled: doc.visible && !ctl.review_mode; onActivated: doc.draftReads() }
    Shortcut { sequence: "Ctrl+Alt+Home"; enabled: doc.visible && ctl.review_mode; onActivated: doc.setMaskStart(ctl.selected_column) }
    Shortcut { sequence: "Ctrl+Alt+End"; enabled: doc.visible && ctl.review_mode; onActivated: doc.setMaskEnd(ctl.selected_column) }

    function setMaskStart(column) {
        const mask = JSON.parse(ctl.mask_settings)
        ctl.mask_ends(column, mask.end || ctl.consensus_columns)
        view.forceActiveFocus()
    }
    function setMaskEnd(column) {
        const mask = JSON.parse(ctl.mask_settings)
        ctl.mask_ends((mask.start || 0) + 1, column)
        view.forceActiveFocus()
    }
    function maskMenuAt(x, y) {
        const column = ctl.context_column(x, y)
        if (!column) return
        maskMenu.column = column
        traceMouse.siteText = ""
        maskMenu.popup(x, y)
    }


    readonly property var traceRows: ctl.trace_lanes.length ? JSON.parse(ctl.trace_lanes) : []
    property var returnAlignment: null
    function startReview(paths, reference, target, cutoff = 20) {
        sourceInitialized = true
        returnAlignment = target
        reviewReference = reference
        if (!ctl.open_file(paths[0])) return
        const ready = paths.length === 1 ? ctl.single_review(cutoff) : ctl.draft_consensus(JSON.stringify(paths.slice(1)), cutoff)
        if (ready && reference.length && ctl.orient_review(reference)) ctl.place_alignment(reference)
    }
    function signalSettings() {
        signalDialog.paths = JSON.parse(ctl.input_paths)
        signalDialog.reference = reviewReference
        signalDialog.target = returnAlignment
        signalCutoff.value = ctl.signal_threshold
        signalDialog.open()
    }
    AppContentDialog {
        id: signalDialog
        initialFocusItem: signalCutoff
        property var paths: []
        property string reference: ""
        property var target: null
        title: "Draft signal cutoff"
        modal: true
        body: ColumnLayout {
            spacing: 10
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: "Trim each read's ends when the median signal over eight bases drops below this percentage of the middle half's median signal. Lower values retain more of the ends. This measures signal strength, not base-call confidence."
            }
            RowLayout {
                Label { id: signalCutoffLabel; text: "Signal cutoff (%)" }
                SpinBox { id: signalCutoff; objectName: "signalCutoff"; Accessible.labelledBy: signalCutoffLabel; Accessible.name: signalCutoffLabel.text; from: 1; to: 90; value: 20; editable: true }
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: ctl.review_mode ? "Current draft: " + ctl.signal_threshold + "%. The new draft opens in another tab; this review keeps its decisions and masks." : "Default: 20%. The chosen value is recorded with the draft."
            }
            RowLayout {
                Item { Layout.fillWidth: true }
                AppButton { text: "Cancel"; onClicked: signalDialog.close() }
                AppButton {
                    text: "Create draft"
                    onClicked: {
                        signalDialog.close()
                        if (ctl.review_mode) doc.redraft(signalDialog.paths, signalDialog.reference, signalDialog.target, signalCutoff.value)
                        else doc.startReview(signalDialog.paths, signalDialog.reference, signalDialog.target, signalCutoff.value)
                    }
                }
            }
        }
    }
    function addToAlignment() {
        const sequence = ctl.reviewed_sequence()
        if (sequence.length && returnAlignment) returnAlignment.addReviewed(sequence, doc.specimenId)
    }
    function maskEnds() {
        const mask = JSON.parse(ctl.mask_settings)
        keepStart.value = (mask.start || 0) + 1
        keepEnd.value = mask.end || ctl.consensus_columns
        maskDialog.open()
    }
    function setPrimers() {
        const mask = JSON.parse(ctl.mask_settings)
        forwardPrimer.text = mask.forward || ""
        reversePrimer.text = mask.reverse || ""
        primerOverlap.value = mask.minimum_overlap || 12
        primerErrors.value = mask.error_percent === undefined ? 25 : mask.error_percent
        primerDialog.proposal = ""
        primerDialog.open()
    }
    AppContentDialog {
        id: maskDialog
        initialFocusItem: keepStart
        title: "Mask consensus ends"
        modal: true
        body: ColumnLayout {
            spacing: 8
            Label { text: "Keep the following consensus columns; masked ends stay visible and are excluded from every export."; wrapMode: Text.Wrap; Layout.fillWidth: true }
            RowLayout {
                Label { id: keepStartLabel; text: "Keep from" }
                SpinBox { id: keepStart; objectName: "keepStart"; Accessible.labelledBy: keepStartLabel; Accessible.name: keepStartLabel.text; from: 1; to: ctl.consensus_columns; editable: true }
                AppButton { text: "Selected column"; onClicked: keepStart.value = ctl.selected_column }
            }
            RowLayout {
                Label { id: keepEndLabel; text: "Keep through" }
                SpinBox { id: keepEnd; objectName: "keepEnd"; Accessible.labelledBy: keepEndLabel; Accessible.name: keepEndLabel.text; from: 1; to: ctl.consensus_columns; editable: true }
                AppButton { text: "Selected column"; onClicked: keepEnd.value = ctl.selected_column }
            }
            RowLayout {
                AppButton { text: "Outside reference"; onClicked: if (ctl.mask_reference()) { maskDialog.close(); view.forceActiveFocus() } }
                AppButton { text: "Clear masks"; onClicked: { keepStart.value = 1; keepEnd.value = ctl.consensus_columns } }
            }
            AppErrorLabel { text: ctl.message; visible: text.length > 0; wrapMode: Text.Wrap; Layout.fillWidth: true; color: ctl.accent }
            RowLayout {
                Item { Layout.fillWidth: true }
                AppButton { text: "Cancel"; onClicked: maskDialog.close() }
                AppButton { text: "Apply mask"; enabled: keepStart.value <= keepEnd.value; onClicked: if (ctl.mask_ends(keepStart.value, keepEnd.value)) { maskDialog.close(); view.forceActiveFocus() } }
            }
        }
    }
    AppContentDialog {
        id: primerDialog
        initialFocusItem: forwardPrimer
        property string proposal: ""
        title: "Set primers"
        modal: true
        body: ColumnLayout {
            spacing: 8
            Label { text: "Enter the primer oligos 5′ to 3′. Either amplicon orientation is tested; candidates are searched within 120 bases of each end."; wrapMode: Text.Wrap; Layout.fillWidth: true }
            Label { id: forwardPrimerLabel; text: "Forward primer" }
            TextField { id: forwardPrimer; objectName: "forwardPrimer"; Accessible.labelledBy: forwardPrimerLabel; Accessible.name: forwardPrimerLabel.text; Layout.fillWidth: true; placeholderText: "IUPAC DNA"; onTextEdited: primerDialog.proposal = "" }
            Label { id: reversePrimerLabel; text: "Reverse primer" }
            TextField { id: reversePrimer; objectName: "reversePrimer"; Accessible.labelledBy: reversePrimerLabel; Accessible.name: reversePrimerLabel.text; Layout.fillWidth: true; placeholderText: "IUPAC DNA"; onTextEdited: primerDialog.proposal = "" }
            RowLayout {
                Label { id: primerOverlapLabel; text: "Minimum overlap" }
                SpinBox { id: primerOverlap; objectName: "primerOverlap"; Accessible.labelledBy: primerOverlapLabel; Accessible.name: primerOverlapLabel.text; from: 8; to: 100; value: 12; editable: true; onValueModified: primerDialog.proposal = "" }
                Label { id: primerErrorsLabel; text: "Max errors (%)" }
                SpinBox { id: primerErrors; objectName: "primerErrors"; Accessible.labelledBy: primerErrorsLabel; Accessible.name: primerErrorsLabel.text; from: 0; to: 40; value: 25; editable: true; onValueModified: primerDialog.proposal = "" }
            }
            AppButton { text: "Find primer candidates"; onClicked: primerDialog.proposal = ctl.find_primers(forwardPrimer.text, reversePrimer.text, primerOverlap.value, primerErrors.value) }
            Label {
                text: primerDialog.proposal.length ? JSON.parse(primerDialog.proposal).hits.map(h => h.side + "′ candidate: bases " + (h.start + 1) + "–" + h.end + ", overlap " + h.overlap + ", errors " + h.errors).join("\n") || "No convincing terminal primer candidates" : ""
                visible: text.length > 0; wrapMode: Text.Wrap; Layout.fillWidth: true
            }
            AppErrorLabel { text: ctl.message; visible: text.length > 0; wrapMode: Text.Wrap; Layout.fillWidth: true; color: ctl.accent }
            RowLayout {
                Item { Layout.fillWidth: true }
                AppButton { text: "Cancel"; onClicked: primerDialog.close() }
                AppButton { text: "Apply primer masks"; enabled: primerDialog.proposal.length > 0; onClicked: if (ctl.apply_primers(primerDialog.proposal)) { primerDialog.close(); view.forceActiveFocus() } }
            }
        }
    }
    Shortcut { sequence: "Ctrl+Alt+M"; enabled: doc.visible && ctl.review_mode; onActivated: doc.maskEnds() }
    Shortcut { sequence: "Ctrl+Alt+P"; enabled: doc.visible && ctl.review_mode; onActivated: doc.setPrimers() }
    SangerController { id: ctl }
    Shortcut { sequence: "F2"; enabled: doc.visible && ctl.review_mode; onActivated: doc.beginExport("fasta") }
    function localPath(url) { return fileUrls.local_file_path(url) }
    FileDialog {
        currentFolder: doc.activeFolder()
        onVisibleChanged: if (visible) currentFolder = doc.activeFolder()
        id: draftDialog
        title: "Open draft evidence"
        nameFilters: ["Draft evidence (*.json)", "All files (*)"]
        onAccepted: ctl.open_draft(doc.localPath(selectedFile))
    }
    FileDialog {
        currentFolder: doc.activeFolder()
        onVisibleChanged: if (visible) currentFolder = doc.activeFolder()
        id: openReviewDialog
        title: "Open Sanger review"
        nameFilters: ["Sanger review (*.sanger-review.json)", "JSON files (*.json)"]
        onAccepted: ctl.open_review(doc.localPath(selectedFile))
    }
    FileDialog {
        currentFolder: doc.activeFolder()
        onVisibleChanged: if (visible) currentFolder = doc.activeFolder()
        id: saveReviewDialog
        title: "Save Sanger review"
        fileMode: FileDialog.SaveFile
        defaultSuffix: "json"
        nameFilters: ["Sanger review (*.sanger-review.json)"]
        onAccepted: ctl.save_review(doc.localPath(selectedFile))
    }
    FileDialog {
        currentFolder: doc.activeFolder()
        onVisibleChanged: if (visible) currentFolder = doc.activeFolder()
        id: alignmentDialog
        title: "Choose frozen source alignment"
        nameFilters: ["Aligned FASTA (*.afa *.fasta *.fa *.fas *.fna)"]
        onAccepted: {
            const path = doc.localPath(selectedFile)
            if (ctl.orient_review(path) && ctl.place_alignment(path)) doc.reviewReference = path
        }
    }
    FileDialog {
        currentFolder: doc.activeFolder()
        onVisibleChanged: if (visible) currentFolder = doc.activeFolder()
        id: exportConsensusDialog
        title: "Export reviewed consensus"
        fileMode: FileDialog.SaveFile
        defaultSuffix: "fasta"
        nameFilters: ["FASTA sequence (*.fa *.fasta)"]
        onAccepted: ctl.export_consensus(doc.localPath(selectedFile), doc.specimenId)
    }
    FileDialog {
        currentFolder: doc.activeFolder()
        onVisibleChanged: if (visible) currentFolder = doc.activeFolder()
        id: additionalDialog
        title: "Select one or two more reads from the same specimen and amplicon"
        fileMode: FileDialog.OpenFiles
        nameFilters: ["Sanger traces (*.ab1 *.abi)"]
        onAccepted: {
            const paths = selectedFiles.map(url => doc.localPath(url))
            doc.startReview([doc.path].concat(paths), "", null)
        }
    }
    Component.onCompleted: Qt.callLater(() => {
        ctl.resize(view.width, view.height)
        if (!doc.sourceInitialized) {
            doc.sourceInitialized = true
            ctl.open_file(doc.path)
        }
    })

    component CompactSpinBox: SpinBox {
        id: compact
        implicitWidth: Math.max(100, implicitContentWidth + leftPadding + rightPadding)
        leftPadding: 22; rightPadding: 22; topPadding: 1; bottomPadding: 1
        up.indicator: AppIcon {
            x: compact.width - width; width: 20; height: compact.height
            name: "add"; color: ctl.foreground; size: 12
            opacity: compact.value >= compact.to ? 0.35 : 1
        }
        down.indicator: AppIcon {
            width: 20; height: compact.height
            name: "minus"; color: ctl.foreground; size: 12
            opacity: compact.value <= compact.from ? 0.35 : 1
        }
    }

    Flickable {
        id: viewport
        anchors.fill: parent
        anchors.bottomMargin: cutoffBar.height + (ctl.review_mode && ctl.hscroll_size < 0.999 ? 14 : 0)
        contentWidth: width
        contentHeight: view.height
        clip: true
        interactive: false
        ScrollBar.vertical: ScrollBar { policy: ctl.review_mode && viewport.contentHeight > viewport.height ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff }
    AppCanvas {
        id: view
        accessibleTitle: "Sanger trace editor"
        accessibleSummary: ctl.status
        width: viewport.width - (ctl.review_mode && viewport.contentHeight > viewport.height ? 14 : 0)
        height: Math.max(viewport.height, ctl.review_mode ? ctl.review_height : 0)
        clearColor: ctl.background
        fontFamily: ctl.mono_font
        instances: ctl.frame
        focus: true
        onWidthChanged: ctl.resize(view.width, view.height)
        onHeightChanged: ctl.resize(view.width, view.height)

        Menu {
            id: maskMenu
            implicitWidth: Math.max(320, maskStartItem.implicitWidth, maskEndItem.implicitWidth)
            property int column: 0
            readonly property var mask: JSON.parse(ctl.mask_settings)
            MenuItem { text: "Consensus column " + maskMenu.column; enabled: false }
            MenuSeparator {}
            MenuItem {
                id: maskStartItem
                text: "Keep from here    Ctrl+Alt+Home"
                enabled: maskMenu.column <= (maskMenu.mask.end || ctl.consensus_columns)
                onTriggered: doc.setMaskStart(maskMenu.column)
            }
            MenuItem {
                id: maskEndItem
                text: "Keep through here    Ctrl+Alt+End"
                enabled: maskMenu.column >= (maskMenu.mask.start || 0) + 1
                onTriggered: doc.setMaskEnd(maskMenu.column)
            }
            onClosed: view.forceActiveFocus()
        }

        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.MiddleButton | Qt.RightButton
            hoverEnabled: ctl.review_mode
            property string siteText: ""
            property real lastY: 0
            property real lastX: 0
            onPressed: (mouse) => {
                view.forceActiveFocus()
                lastX = mouse.x
                if (mouse.button === Qt.LeftButton) ctl.press(mouse.x, mouse.y)
                else if (mouse.button === Qt.RightButton && ctl.review_mode) doc.maskMenuAt(mouse.x, mouse.y)
            }
            id: traceMouse
            onPositionChanged: (mouse) => {
                lastY = mouse.y
                if (mouse.buttons & Qt.MiddleButton) { ctl.pan(mouse.x - lastX); siteText = "" }
                else if (ctl.review_mode) doc.hoverAt(mouse.x, mouse.y)
                lastX = mouse.x
            }
            onExited: { siteText = ""; ctl.hover(-1, -1) }
            onWheel: (wheel) => {
                siteText = ""
                if (wheel.modifiers & Qt.ControlModifier) ctl.wheel(wheel.x, wheel.angleDelta.y)
                else if (ctl.review_mode && ((wheel.modifiers & Qt.ShiftModifier) || Math.abs(wheel.angleDelta.x) > Math.abs(wheel.angleDelta.y))) ctl.pan((wheel.angleDelta.x || wheel.angleDelta.y) * 0.7)
                else if (ctl.review_mode) viewport.contentY = Math.max(0, Math.min(viewport.contentHeight - viewport.height, viewport.contentY - wheel.angleDelta.y))
                else ctl.wheel(wheel.x, wheel.angleDelta.y)
            }
        }
        Repeater {
            model: doc.traceRows.length
            Column {
                required property int index
                x: 8; y: doc.traceRows[index].top + 2
                spacing: 1
                Label { id: gainLabel; text: "vertical zoom"; color: ctl.foreground }
                CompactSpinBox {
                    Accessible.labelledBy: gainLabel
                    Accessible.name: gainLabel.text + " (%)"
                    from: 25; to: 2000; stepSize: 25; editable: true
                    value: Math.round(doc.traceRows[index].gain * 100)
                    validator: RegularExpressionValidator { regularExpression: /^\d+\s*%?$/ }
                    textFromValue: value => value + "%"
                    valueFromText: text => parseInt(text)
                    onValueModified: ctl.trace_gain(index, value / 100)
                    ToolTip.visible: hovered
                    ToolTip.text: "Signal magnification; enlarged peaks are clipped at the top"
                }
                Label { id: heightLabel; text: "row height"; color: ctl.foreground }
                CompactSpinBox {
                    Accessible.labelledBy: heightLabel
                    Accessible.name: heightLabel.text + " (px)"
                    from: 80; to: 1200; stepSize: 20; editable: true
                    value: doc.traceRows[index].height
                    validator: RegularExpressionValidator { regularExpression: /^\d+\s*(px)?$/ }
                    textFromValue: value => value + " px"
                    valueFromText: text => parseInt(text)
                    onValueModified: ctl.trace_height(index, value)
                    ToolTip.visible: hovered
                    ToolTip.text: "Trace height on screen"
                }
            }
        }
        ScrollBar {
            id: traceScroll
            policy: ScrollBar.AlwaysOn
            width: 12
            contentItem: Rectangle { implicitWidth: 10; radius: 5; color: Qt.alpha(ctl.foreground, traceScroll.pressed ? 0.65 : 0.4) }
            background: Rectangle { color: Qt.alpha(ctl.foreground, 0.06) }
            visible: !ctl.review_mode && ctl.trace_scroll_size < 0.999
            orientation: Qt.Vertical
            anchors.top: parent.top
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            size: ctl.trace_scroll_size
            minimumSize: 0.04
            onPositionChanged: if (pressed) ctl.scroll_trace(position)
        }
        Binding { target: traceScroll; property: "position"; value: ctl.trace_scroll_pos; when: !traceScroll.pressed; restoreMode: Binding.RestoreNone }
        Keys.onPressed: (event) => {
            if (!view.renderingAvailable) { event.accepted = false; return }
            if (ctl.review_mode && !(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)) &&
                    (event.key === Qt.Key_Delete || /^[ACGTNRYKMSWBDHV-]$/i.test(event.text))) {
                if (ctl.review_set("edit", event.key === Qt.Key_Delete ? "-" : event.text)) ctl.next_call(true)
                event.accepted = true
            }
            else if (ctl.review_mode && event.matches(StandardKey.Undo)) { ctl.review_undo(); event.accepted = true }
            else if (ctl.review_mode && event.matches(StandardKey.Redo)) { ctl.review_redo(); event.accepted = true }
            else if (event.key === Qt.Key_Right) { ctl.next_call(true); event.accepted = true }
            else if (event.key === Qt.Key_Left) { ctl.next_call(false); event.accepted = true }
            else if (event.key === Qt.Key_BracketRight) { doc.issue(true); event.accepted = true }
            else if (event.key === Qt.Key_BracketLeft) { doc.issue(false); event.accepted = true }
        }
    }
    } // viewport
    ScrollBar {
        id: reviewScroll
        policy: ScrollBar.AlwaysOn
        height: 14
        contentItem: Rectangle { implicitHeight: 10; radius: 5; color: Qt.alpha(ctl.foreground, reviewScroll.pressed ? 0.65 : 0.4) }
        background: Rectangle { color: Qt.alpha(ctl.foreground, 0.06) }
        visible: ctl.review_mode && ctl.hscroll_size < 0.999
        orientation: Qt.Horizontal
        anchors.left: parent.left
        anchors.leftMargin: 180
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.bottomMargin: cutoffBar.height
        size: ctl.hscroll_size
        minimumSize: 0.04
        onPositionChanged: if (pressed) ctl.hscroll_to(position)
    }
    Binding { target: reviewScroll; property: "position"; value: ctl.hscroll_pos; when: !reviewScroll.pressed; restoreMode: Binding.RestoreNone }
    Flickable {
        id: cutoffBar
        objectName: "traceCutoffBar"
        visible: ctl.review_mode
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: visible ? cutoffs.implicitHeight + 12 : 0
        clip: true
        contentWidth: Math.max(width, cutoffs.implicitWidth + 16)
        contentHeight: height
        interactive: contentWidth > width
        flickableDirection: Flickable.HorizontalFlick
        ScrollBar.horizontal: ScrollBar { policy: ScrollBar.AsNeeded }
        TraceReviewControls {
            id: cutoffs
            x: 8
            y: 2
            width: Math.max(cutoffBar.width - 16, implicitWidth)
            controller: ctl
        }
    }

}
