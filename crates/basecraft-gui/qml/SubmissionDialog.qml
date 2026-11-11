import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// A GenBank marker submission: batch settings, authors and title, a
// decision and reviewed reading frame per row, every issue (selecting one
// selects its row), the package and its table2asn validation, and returned
// accessions. Each change is one undoable edit, saved at once by the
// controller.
AppDialog {
    id: dialog
    objectName: "submissionDialog"
    title: "GenBank submission"
    standardButtons: Dialog.Close
    initialFocusItem: sequenceSearch
    // SubmissionController.document, .review and .package, parsed.
    property var document: null
    property var review: ({
            rows: [],
            issues: []
        })
    property var pkg: null
    property var geneticCodes: []
    property string path: ""
    property string message: ""
    property string messageKind: ""
    property bool canUndo: false
    property bool canRedo: false
    // A table2asn job is running.
    property bool validating: false
    property bool reviewing: false
    property string reviewStage: "Reviewing submission"
    property string validationStage: "Validating with table2asn"
    property var inclusionPlan: null
    property var inclusionPresets: []
    property string presetError: ""
    property var parseAuthors: function (text) {
        return ({
                authors: []
            });
    }
    signal detailsRequested(string details)
    signal inclusionPreviewRequested(string rule)
    signal inclusionPresetsRequested(string save)
    property alias body: content
    property alias currentTab: tabs.currentIndex
    property real specimenWidth: cellWidth * 1.5
    property real accessionWidth: cellWidth
    property real evidenceWidth: cellWidth * 2
    property real nameColumnWidth: cellMetrics.averageCharacterWidth * 42
    readonly property var filteredRows: review.rows.filter(r => (!sequenceSearch.text.length || r.record.toLowerCase().includes(sequenceSearch.text.toLowerCase())) && (rowFilter.currentIndex === 0 || (rowFilter.currentIndex === 1 ? r.decision === "new" : rowFilter.currentIndex === 2 ? r.decision !== "new" : r.blocking > 0 || r.review > 0)))
    // The accession list matched against the records, not yet recorded.
    property var accessionPlan: null
    property int selectedRow: -1
    signal editRequested(string edit)
    signal publicationRequested(string title, string authors)
    signal decisionRequested(string record, string decision, string accession)
    signal frameRequested(string record, int codonStart)
    signal confirmDetectedRequested
    signal undoRequested
    signal contactDetailsRequested
    signal redoRequested
    signal exportRequested
    signal validateRequested
    signal cancelValidationRequested
    signal accessionsMatchRequested(string text)
    signal accessionsRecordRequested(string accessions)
    signal accessionTableRequested

    readonly property var batch: document ? document.batch : null
    readonly property string kind: batch ? batch.marker.kind : "rrna"
    readonly property bool coding: kind === "coding"
    readonly property var markerKinds: [["coding", "Protein-coding gene"], ["rrna", "Ribosomal RNA gene"], ["its", "ITS region"], ["spacer", "Non-coding region"]]
    readonly property var organelles: [["", "None (nuclear)"], ["mitochondrion", "Mitochondrion"], ["chloroplast", "Chloroplast"], ["plastid", "Plastid"], ["apicoplast", "Apicoplast"], ["chromoplast", "Chromoplast"], ["cyanelle", "Cyanelle"], ["kinetoplast", "Kinetoplast"], ["nucleomorph", "Nucleomorph"]]
    readonly property var endPolicies: [["auto", "From start and stop codons"], ["partial", "Both partial"], ["complete", "Both complete"]]
    readonly property var itsComponents: [["SSU", "Small subunit rRNA"], ["ITS1", "ITS1"], ["5.8S", "5.8S rRNA"], ["ITS2", "ITS2"], ["LSU", "Large subunit rRNA"]]
    readonly property var severityNames: ({
            blocking: "Must fix",
            review: "Check",
            info: "Note",
            unavailable: "Not checked"
        })

    component ResizeHeading: Label {
        id: heading
        property real columnWidth: 100
        signal resized(real extent)
        Layout.preferredWidth: columnWidth
        elide: Text.ElideRight
        font.bold: true
        activeFocusOnTab: true
        Accessible.name: text + "; resize with left and right arrows"
        Keys.onLeftPressed: resized(Math.max(80, columnWidth - 20))
        Keys.onRightPressed: resized(Math.min(1400, columnWidth + 20))
        MouseArea {
            anchors.right: parent.right
            width: 12
            height: parent.height
            cursorShape: Qt.SplitHCursor
            property real initialWidth: 0
            property real initialX: 0
            onPressed: mouse => {
                initialWidth = heading.columnWidth;
                initialX = mapToItem(dialog.contentItem, mouse.x, 0).x;
            }
            onPositionChanged: mouse => {
                if (pressed)
                    heading.resized(Math.max(80, Math.min(1400, initialWidth + mapToItem(dialog.contentItem, mouse.x, 0).x - initialX)));
            }
        }
    }

    FontMetrics {
        id: cellMetrics
        font: dialog.font
    }
    readonly property real nameWidth: nameColumnWidth
    readonly property real cellWidth: cellMetrics.averageCharacterWidth * 20

    function indexOf(pairs, value) {
        return Math.max(0, pairs.findIndex(p => p[0] === value));
    }
    // The batch settings as the controls show them.
    function batchFromControls() {
        const marker = {
            kind: markerKinds[markerKind.currentIndex][0]
        };
        if (marker.kind === "coding") {
            marker.gene = gene.text.trim();
            marker.product = product.text.trim();
        } else if (marker.kind === "rrna")
            marker.product = product.text.trim();
        else if (marker.kind === "its")
            marker.components = itsComponents.map(c => c[0]).filter(c => itsChecks.checked(c));
        else
            marker.description = product.text.trim();
        const out = {
            marker: marker,
            mol_type: molType.currentIndex === 1 ? "genomic_rna" : "genomic_dna",
            genetic_code: geneticCodes.length ? geneticCodes[geneticCode.currentIndex].id : 1,
            ends: endPolicies[ends.currentIndex][0]
        };
        const location = organelles[organelle.currentIndex][0];
        if (location.length)
            out.organelle = location;
        return out;
    }
    function saveBatch() {
        if (!document)
            return;
        const next = batchFromControls();
        if (JSON.stringify(next) !== JSON.stringify(batch))
            editRequested(JSON.stringify({
                action: "set_batch_draft",
                batch: next
            }));
    }
    function setAllDecisions(decision) {
        editRequested(JSON.stringify({
            action: "set_rows",
            rows: filteredRows.map(r => ({
                        record: r.record,
                        decision: decision
                    }))
        }));
    }
    function selectRow(row) {
        selectedRow = row;
        tabs.currentIndex = 0;
        sequenceSearch.text = "";
        rowFilter.currentIndex = 0;
        Qt.callLater(function () {
            const index = dialog.filteredRows.findIndex(r => r.row === row);
            if (index < 0)
                return;
            sequenceTable.currentIndex = index;
            sequenceTable.positionViewAtIndex(index, ListView.Contain);
            Qt.callLater(function () {
                if (sequenceTable.currentItem)
                    sequenceTable.currentItem.focusDecision();
            });
        });
    }
    function changeMarker() {
        if (!document)
            return;
        const nextKind = markerKinds[markerKind.currentIndex][0];
        const empty = {
            coding: {
                kind: "coding",
                gene: "",
                product: ""
            },
            rrna: {
                kind: "rrna",
                product: ""
            },
            its: {
                kind: "its",
                components: []
            },
            spacer: {
                kind: "spacer",
                description: ""
            }
        };
        const next = Object.assign({}, batch);
        next.marker = document.marker_drafts && document.marker_drafts[nextKind] ? document.marker_drafts[nextKind] : empty[nextKind];
        editRequested(JSON.stringify({
            action: "set_batch_draft",
            batch: next
        }));
    }
    function ruleFromControls() {
        return {
            name: ruleName.text,
            input: ruleSource.currentIndex === 0 ? {
                source: "identifier"
            } : {
                source: "field",
                separator: ruleSeparator.text,
                index: ruleField.value
            },
            mode: ["exact", "contains", "starts", "ends", "wildcard", "regex"][ruleMode.currentIndex],
            pattern: rulePattern.text,
            case_sensitive: ruleCase.checked,
            trim: ruleTrim.checked,
            matching: ["include", "exclude", "unchanged"][ruleMatching.currentIndex],
            nonmatching: ["include", "exclude", "unchanged"][ruleNonmatching.currentIndex],
            unresolved: ["unchanged", "include", "exclude"][ruleUnresolved.currentIndex],
            override_manual: ruleOverride.checked
        };
    }
    function loadRule(rule) {
        ruleName.text = rule.name;
        ruleSource.currentIndex = rule.input.source === "field" ? 1 : 0;
        ruleSeparator.text = rule.input.separator || "";
        ruleField.value = rule.input.index || 1;
        ruleMode.currentIndex = ["exact", "contains", "starts", "ends", "wildcard", "regex"].indexOf(rule.mode);
        rulePattern.text = rule.pattern;
        ruleCase.checked = rule.case_sensitive;
        ruleTrim.checked = rule.trim;
        ruleMatching.currentIndex = ["include", "exclude", "unchanged"].indexOf(rule.matching);
        ruleNonmatching.currentIndex = ["include", "exclude", "unchanged"].indexOf(rule.nonmatching);
        ruleUnresolved.currentIndex = ["unchanged", "include", "exclude"].indexOf(rule.unresolved || "unchanged");
        ruleOverride.checked = rule.override_manual;
    }
    onOpened: inclusionPresetsRequested("")
    onDocumentChanged: inclusionPlan = null

    function frameText(frame) {
        if (!frame)
            return "";
        if (frame.chosen)
            return "codon_start " + frame.chosen + (frame.chosen === frame.detected ? " (confirmed)" : " (chosen)");
        if (frame.detected)
            return "Detected " + frame.detected + ": confirm";
        if (frame.candidates.length)
            return "Choose: " + frame.candidates.join(" or ");
        return "No clean frame";
    }

    contentItem: ColumnLayout {
        id: content
        Shortcut {
            sequences: [StandardKey.Undo]
            enabled: dialog.opened && dialog.canUndo
            onActivated: dialog.undoRequested()
        }
        Shortcut {
            sequences: [StandardKey.Redo]
            enabled: dialog.opened && dialog.canRedo
            onActivated: dialog.redoRequested()
        }
        implicitWidth: 1200
        implicitHeight: Math.min(680, Math.max(240, (Overlay.overlay ? Overlay.overlay.height : 800) - 160))
        spacing: 8
        Label {
            text: dialog.path + " · " + (dialog.review.exported || 0) + " included / " + dialog.review.rows.length + " total · " + (dialog.review.blocking || 0) + " to fix"
            Layout.fillWidth: true
            elide: Text.ElideMiddle
            objectName: "submissionStatus"
        }
        AppErrorLabel {
            objectName: "submissionMessage"
            text: dialog.review.error || dialog.presetError || dialog.message
            visible: text.length > 0
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        AppMessageBar {
            Layout.fillWidth: true
            visible: dialog.reviewing
            busy: dialog.reviewing
            cancellable: false
            workingText: dialog.reviewStage
        }
        TabBar {
            id: tabs
            objectName: "submissionTabs"
            Layout.fillWidth: true
            TabButton {
                text: "Sequences"
            }
            TabButton {
                text: "Annotation"
            }
            TabButton {
                text: "Submission details"
            }
            TabButton {
                text: "Validate & export"
            }
        }
        StackLayout {
            currentIndex: tabs.currentIndex
            Layout.fillWidth: true
            Layout.fillHeight: true
            ColumnLayout {
                Label {
                    objectName: "selectionStatus"
                    text: dialog.review.selection_confirmed ? "Row selection confirmed." : "Review the suggested choices, then confirm the row selection before export. Accession candidates are unverified heuristics; no archive lookup is performed."
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
                RowLayout {
                    spacing: 6
                    AppButton {
                        objectName: "includeAllRows"
                        text: "Include filtered"
                        enabled: !!dialog.review.selection_hash && !dialog.validating && !dialog.reviewing
                        onClicked: dialog.setAllDecisions("new")
                    }
                    AppButton {
                        objectName: "excludeAllRows"
                        text: "Exclude filtered"
                        enabled: !!dialog.review.selection_hash && !dialog.validating && !dialog.reviewing
                        onClicked: dialog.setAllDecisions("exclude")
                    }
                    AppButton {
                        objectName: "confirmSelection"
                        text: "Confirm row selection"
                        enabled: !!dialog.review.selection_hash && !dialog.review.selection_confirmed && !dialog.validating && !dialog.reviewing
                        onClicked: dialog.editRequested(JSON.stringify({
                            action: "confirm_selection",
                            selection_hash: dialog.review.selection_hash
                        }))
                    }
                }

                RowLayout {
                    Label {
                        text: "Search"
                    }
                    TextField {
                        id: sequenceSearch
                        objectName: "sequenceSearch"
                        placeholderText: "Search identifiers"
                        Accessible.name: "Search identifiers"
                        Layout.fillWidth: true
                    }
                    AppComboBox {
                        id: rowFilter
                        model: ["All sequences", "Included", "Excluded", "With issues"]
                        Accessible.name: "Filter sequences"
                    }
                    Label {
                        text: dialog.filteredRows.length + " shown"
                    }
                    AppCheckBox {
                        id: showRule
                        text: "Configure inclusion rule"
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    AppCheckBox {
                        id: showAccessionSource
                        text: "Configure accession evidence source (suggestions only)"
                    }
                    ColumnLayout {
                        visible: showAccessionSource.checked
                        Layout.fillWidth: true
                        Label {
                            id: accessionExplanation
                            text: "Set the source before reviewing default suggestions. Explicit inclusion decisions are preserved. Syntactic matches do not verify archive identity."
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                        }
                        GridLayout {
                            id: accessionPolicyForm
                            columns: width > dialog.cellWidth * 4 ? 3 : 1
                            Layout.fillWidth: true
                            columnSpacing: 12
                            rowSpacing: 8
                            ColumnLayout {
                                Label {
                                    text: "Identifier evidence"
                                }
                                AppComboBox {
                                    id: accessionInput
                                    Accessible.name: "Accession identifier evidence"
                                    model: ["Whole identifier heuristic", "Chosen identifier field", "No identifier evidence"]
                                    currentIndex: !dialog.document || !dialog.document.accession_policy || dialog.document.accession_policy.policy === "legacy_heuristic" ? 0 : !dialog.document.accession_policy.input ? 2 : dialog.document.accession_policy.input.source === "field" ? 1 : 0
                                }
                            }
                            ColumnLayout {
                                visible: accessionInput.currentIndex === 1
                                Label {
                                    text: "Literal separator"
                                }
                                TextField {
                                    id: accessionSeparator
                                    Accessible.name: "Accession separator"
                                    text: dialog.document && dialog.document.accession_policy && dialog.document.accession_policy.input ? dialog.document.accession_policy.input.separator || "" : ""
                                }
                            }
                            ColumnLayout {
                                visible: accessionInput.currentIndex === 1
                                Label {
                                    text: "Field (one-based)"
                                }
                                SpinBox {
                                    id: accessionField
                                    from: 1
                                    to: 1000
                                    editable: true
                                    Accessible.name: "Accession field"
                                    value: dialog.document && dialog.document.accession_policy && dialog.document.accession_policy.input ? dialog.document.accession_policy.input.index || 1 : 1
                                }
                            }
                            AppCheckBox {
                                id: accessionMetadata
                                text: "Also scan specimen metadata (heuristic)"
                                checked: !dialog.document || !dialog.document.accession_policy || dialog.document.accession_policy.policy === "legacy_heuristic" || dialog.document.accession_policy.include_metadata
                            }
                            AppButton {
                                text: "Set evidence source"
                                onClicked: dialog.editRequested(JSON.stringify({
                                    action: "set_accession_policy",
                                    policy: {
                                        policy: "configured",
                                        input: accessionInput.currentIndex === 2 ? null : accessionInput.currentIndex === 0 ? {
                                            source: "identifier"
                                        } : {
                                            source: "field",
                                            separator: accessionSeparator.text,
                                            index: accessionField.value
                                        },
                                        include_metadata: accessionMetadata.checked
                                    }
                                }))
                            }
                        }
                    }
                }
                FocusScrollView {
                    visible: showRule.checked
                    Layout.fillWidth: true
                    Layout.preferredHeight: Math.min(210, ruleForm.implicitHeight)
                    contentWidth: availableWidth
                    ColumnLayout {
                        id: ruleForm
                        width: parent.width
                        Label {
                            text: "Match FASTA identifiers (before whitespace). Fields use a literal separator; quotes/escapes are not interpreted."
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                        }
                        GridLayout {
                            columns: width > dialog.cellWidth * 4 ? 3 : 1
                            Layout.fillWidth: true
                            columnSpacing: 12
                            rowSpacing: 8
                            ColumnLayout {
                                Label {
                                    text: "Rule / preset name"
                                }
                                TextField {
                                    id: ruleName
                                    text: "Identifier inclusion"
                                    Accessible.name: "Rule name"
                                }
                            }
                            ColumnLayout {
                                Label {
                                    text: "Saved preset"
                                }
                                AppComboBox {
                                    model: ["Choose preset"].concat(dialog.inclusionPresets.map(r => r.name))
                                    onActivated: index => {
                                        if (index > 0)
                                            dialog.loadRule(dialog.inclusionPresets[index - 1]);
                                    }
                                }
                            }
                            ColumnLayout {
                                Label {
                                    text: "Input"
                                }
                                AppComboBox {
                                    id: ruleSource
                                    model: ["Whole identifier", "Identifier field"]
                                    Accessible.name: "Rule input"
                                }
                            }
                            ColumnLayout {
                                visible: ruleSource.currentIndex === 1
                                Label {
                                    text: "Literal separator"
                                }
                                TextField {
                                    id: ruleSeparator
                                    Accessible.name: "Literal separator"
                                }
                            }
                            ColumnLayout {
                                visible: ruleSource.currentIndex === 1
                                Label {
                                    text: "Field (one-based)"
                                }
                                SpinBox {
                                    id: ruleField
                                    from: 1
                                    to: 1000
                                    editable: true
                                    Accessible.name: "One-based field"
                                }
                            }
                        }
                        GridLayout {
                            columns: width > dialog.cellWidth * 4 ? 3 : 1
                            Layout.fillWidth: true
                            columnSpacing: 12
                            rowSpacing: 8
                            ColumnLayout {
                                Label {
                                    text: "Matching mode"
                                }
                                AppComboBox {
                                    id: ruleMode
                                    model: ["Exact", "Contains", "Starts with", "Ends with", "Wildcard (*, ?)", "Regular expression"]
                                    Accessible.name: "Matching mode"
                                }
                            }
                            ColumnLayout {
                                Label {
                                    text: "Pattern"
                                }
                                TextField {
                                    id: rulePattern
                                    Accessible.name: "Rule pattern"
                                }
                            }
                            AppCheckBox {
                                id: ruleCase
                                text: "Case sensitive"
                                checked: true
                            }
                            AppCheckBox {
                                id: ruleTrim
                                text: "Trim field whitespace"
                                checked: true
                            }
                        }
                        GridLayout {
                            columns: width > dialog.cellWidth * 4 ? 3 : 1
                            Layout.fillWidth: true
                            columnSpacing: 12
                            rowSpacing: 8
                            ColumnLayout {
                                Label {
                                    text: "Matching rows"
                                }
                                AppComboBox {
                                    id: ruleMatching
                                    model: ["Include", "Exclude", "Leave unchanged"]
                                }
                            }
                            ColumnLayout {
                                Label {
                                    text: "Nonmatching rows"
                                }
                                AppComboBox {
                                    id: ruleNonmatching
                                    model: ["Include", "Exclude", "Leave unchanged"]
                                    currentIndex: 1
                                }
                            }
                            ColumnLayout {
                                Label {
                                    text: "Missing / empty field"
                                }
                                AppComboBox {
                                    id: ruleUnresolved
                                    model: ["Leave unchanged", "Include", "Exclude"]
                                }
                            }
                            AppCheckBox {
                                id: ruleOverride
                                text: "Replace manual decisions"
                            }
                            AppButton {
                                text: "Preview rule"
                                objectName: "previewInclusion"
                                onClicked: {
                                    dialog.inclusionPreviewRequested(JSON.stringify(dialog.ruleFromControls()));
                                    if (dialog.inclusionPlan && !dialog.inclusionPlan.error)
                                        inclusionPreview.open();
                                }
                            }
                            AppButton {
                                text: "Save preset"
                                onClicked: dialog.inclusionPresetsRequested(JSON.stringify(dialog.ruleFromControls()))
                            }
                        }
                    }
                }
                Item {
                    Layout.fillWidth: true
                    implicitHeight: sequenceHeadings.implicitHeight
                    clip: true
                    RowLayout {
                        id: sequenceHeadings
                        x: -sequenceTable.contentX
                        width: sequenceTable.contentWidth
                        spacing: 4
                        Label {
                            text: "Include"
                            Layout.preferredWidth: dialog.cellWidth * 0.5
                        }
                        ResizeHeading {
                            text: "Identifier"
                            columnWidth: dialog.nameWidth
                            onResized: extent => dialog.nameColumnWidth = extent
                        }
                        ResizeHeading {
                            text: "Specimen / organism"
                            columnWidth: dialog.specimenWidth
                            onResized: extent => dialog.specimenWidth = extent
                        }
                        ResizeHeading {
                            text: "Accession evidence"
                            columnWidth: dialog.accessionWidth
                            onResized: extent => dialog.accessionWidth = extent
                        }
                        ResizeHeading {
                            text: "Selection reason / evidence"
                            columnWidth: dialog.evidenceWidth
                            onResized: extent => dialog.evidenceWidth = extent
                        }
                        Label {
                            visible: dialog.coding
                            text: "Reading frame"
                            Layout.preferredWidth: dialog.cellWidth * 1.5
                        }
                        Label {
                            text: "Issues"
                            Layout.fillWidth: true
                        }
                    }
                }
                ListView {
                    id: sequenceTable
                    objectName: "submissionSequences"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.minimumHeight: 100
                    clip: true
                    reuseItems: true
                    model: dialog.filteredRows
                    contentWidth: dialog.nameWidth + dialog.specimenWidth + dialog.accessionWidth + dialog.evidenceWidth + dialog.cellWidth * (dialog.coding ? 2 : 0.5) + 124
                    flickableDirection: Flickable.AutoFlickDirection
                    ScrollBar.vertical: ScrollBar {}
                    ScrollBar.horizontal: ScrollBar {}
                    delegate: RowLayout {
                        id: row
                        width: sequenceTable.contentWidth
                        height: Math.max(42, implicitHeight)
                        required property var modelData
                        required property int index
                        readonly property bool selected: dialog.selectedRow === modelData.row
                        spacing: 4
                        function focusDecision() {
                            decision.forceActiveFocus();
                        }
                        AppCheckBox {
                            id: decision
                            objectName: "decision_" + row.modelData.record
                            Accessible.name: "Include " + row.modelData.record
                            checked: row.modelData.decision === "new"
                            enabled: !dialog.validating
                            Layout.preferredWidth: dialog.cellWidth * 0.5
                            onActiveFocusChanged: if (activeFocus)
                                dialog.selectedRow = row.modelData.row
                            onToggled: dialog.decisionRequested(row.modelData.record, checked ? "new" : "exclude", "")
                        }
                        Label {
                            text: row.modelData.record
                            ToolTip.visible: nameHover.hovered
                            ToolTip.text: text
                            HoverHandler {
                                id: nameHover
                            }
                            font.bold: row.selected
                            elide: Text.ElideRight
                            Layout.preferredWidth: dialog.nameWidth
                        }
                        Label {
                            text: row.modelData.specimen.length ? row.modelData.specimen + " · " + row.modelData.organism : row.modelData.decision === "new" ? "Not linked" : ""
                            elide: Text.ElideRight
                            Layout.preferredWidth: dialog.specimenWidth
                            ToolTip.text: row.modelData.research_name.length ? "Research name: " + row.modelData.research_name : ""
                        }
                        Label {
                            objectName: "accession_" + row.modelData.record
                            text: row.modelData.accession || (row.modelData.accession_candidates || []).map(c => c.accession).join(", ")
                            Layout.preferredWidth: dialog.accessionWidth
                            elide: Text.ElideRight
                            ToolTip.visible: accessionHover.hovered
                            HoverHandler {
                                id: accessionHover
                            }
                            ToolTip.text: "Accession evidence is unverified unless returned by the archive. Inclusion does not verify an accession."
                        }
                        Label {
                            objectName: "selectionReason_" + row.modelData.record
                            text: (row.modelData.selection_reason || "") + (row.modelData.accession_candidates || []).map(c => "\n" + c.accession + " · " + c.source + " (unverified)").join("")
                            Accessible.name: "Selection reason for " + row.modelData.record
                            elide: Text.ElideRight
                            maximumLineCount: 1
                            ToolTip.visible: evidenceHover.hovered
                            ToolTip.text: text
                            HoverHandler {
                                id: evidenceHover
                            }
                            Layout.preferredWidth: dialog.evidenceWidth
                        }
                        RowLayout {
                            visible: dialog.coding && row.modelData.frame !== null && row.modelData.frame !== undefined
                            spacing: 4
                            Layout.preferredWidth: dialog.cellWidth * 1.5
                            Label {
                                objectName: "frameText_" + row.modelData.record
                                text: dialog.frameText(row.modelData.frame)
                            }
                            AppComboBox {
                                id: frameChoice
                                objectName: "frame_" + row.modelData.record
                                Accessible.name: "Reading frame (codon_start) of " + row.modelData.record
                                model: ["1", "2", "3"]
                                currentIndex: row.modelData.frame && row.modelData.frame.effective ? row.modelData.frame.effective - 1 : 0
                                onFocusChanged: if (activeFocus)
                                    dialog.selectedRow = row.modelData.row
                                onActivated: index => dialog.frameRequested(row.modelData.record, index + 1)
                            }
                            AppButton {
                                objectName: "confirmFrame_" + row.modelData.record
                                text: "Confirm"
                                visible: !!row.modelData.frame && !row.modelData.frame.chosen
                                Accessible.name: "Confirm reading frame of " + row.modelData.record
                                onClicked: dialog.frameRequested(row.modelData.record, frameChoice.currentIndex + 1)
                            }
                        }
                        Label {
                            text: row.modelData.blocking ? row.modelData.blocking + " to fix" : row.modelData.review ? row.modelData.review + " to check" : ""
                        }
                    }
                }
                TextArea {
                    objectName: "translation"
                    readonly property var rowData: dialog.selectedRow >= 0 && dialog.selectedRow < dialog.review.rows.length ? dialog.review.rows[dialog.selectedRow] : null
                    visible: dialog.coding && rowData !== null && !!rowData.frame && rowData.frame.translation.length > 0
                    readOnly: true
                    wrapMode: TextEdit.WrapAnywhere
                    font.family: "monospace"
                    Layout.fillWidth: true
                    Layout.maximumHeight: 90
                    Accessible.name: "Translation of the selected record"
                    text: visible ? rowData.record + " translation (codon_start " + rowData.frame.effective + "): " + rowData.frame.translation : ""
                }
            }
            FocusScrollView {
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    Label {
                        text: "Batch"
                        font.bold: true
                    }
                    GridLayout {
                        columns: width > dialog.cellWidth * 4 ? 3 : 1
                        Layout.fillWidth: true
                        columnSpacing: 12
                        rowSpacing: 8
                        ColumnLayout {
                            Label {
                                text: "Marker type"
                            }
                            AppComboBox {
                                id: markerKind
                                objectName: "markerKind"
                                Accessible.name: "Marker type"
                                model: dialog.markerKinds.map(k => k[1])
                                currentIndex: dialog.indexOf(dialog.markerKinds, dialog.kind)
                                onActivated: dialog.changeMarker()
                            }
                        }
                        ColumnLayout {
                            visible: markerKind.currentIndex === 0
                            Label {
                                text: "Gene symbol"
                            }
                            TextField {
                                id: gene
                                objectName: "markerGene"
                                visible: markerKind.currentIndex === 0
                                placeholderText: "Gene symbol, such as rbcL"
                                Accessible.name: "Gene symbol"
                                text: dialog.batch && dialog.batch.marker.gene !== undefined ? dialog.batch.marker.gene : ""
                                onEditingFinished: dialog.saveBatch()
                            }
                        }
                        ColumnLayout {
                            visible: markerKind.currentIndex !== 2
                            Label {
                                text: "Product / region description"
                            }
                            TextField {
                                id: product
                                objectName: "markerProduct"
                                visible: markerKind.currentIndex !== 2
                                Layout.preferredWidth: dialog.cellWidth * 2
                                placeholderText: markerKind.currentIndex === 3 ? "Region, such as trnL-trnF intergenic spacer" : "Product name"
                                Accessible.name: markerKind.currentIndex === 3 ? "Region description" : "Product name"
                                text: !dialog.batch ? "" : dialog.batch.marker.product !== undefined ? dialog.batch.marker.product : dialog.batch.marker.description !== undefined ? dialog.batch.marker.description : ""
                                onEditingFinished: dialog.saveBatch()
                            }
                        }
                    }
                    RowLayout {
                        id: itsChecks
                        visible: markerKind.currentIndex === 2
                        spacing: 6
                        function checked(component) {
                            for (let i = 0; i < itsRepeater.count; i++)
                                if (dialog.itsComponents[i][0] === component)
                                    return itsRepeater.itemAt(i).checked;
                            return false;
                        }
                        Label {
                            text: "Components present:"
                        }
                        Repeater {
                            id: itsRepeater
                            model: dialog.itsComponents
                            delegate: AppCheckBox {
                                required property var modelData
                                objectName: "its_" + modelData[0]
                                text: modelData[1]
                                checked: dialog.batch && dialog.batch.marker.components !== undefined && dialog.batch.marker.components.includes(modelData[0])
                                onToggled: dialog.saveBatch()
                            }
                        }
                    }
                    GridLayout {
                        columns: width > dialog.cellWidth * 4 ? 3 : 1
                        Layout.fillWidth: true
                        columnSpacing: 12
                        rowSpacing: 8
                        ColumnLayout {
                            Label {
                                text: "Molecule type"
                            }
                            AppComboBox {
                                id: molType
                                Accessible.name: "Molecule type"
                                model: ["Genomic DNA", "Genomic RNA"]
                                currentIndex: dialog.batch && dialog.batch.mol_type === "genomic_rna" ? 1 : 0
                                onActivated: dialog.saveBatch()
                            }
                        }
                        ColumnLayout {
                            Label {
                                text: "Organelle"
                            }
                            AppComboBox {
                                id: organelle
                                objectName: "organelle"
                                Accessible.name: "Organelle"
                                model: dialog.organelles.map(o => o[1])
                                currentIndex: dialog.indexOf(dialog.organelles, dialog.batch && dialog.batch.organelle ? dialog.batch.organelle : "")
                                onActivated: dialog.saveBatch()
                            }
                        }
                        ColumnLayout {
                            Label {
                                text: "Genetic code"
                            }
                            AppComboBox {
                                id: geneticCode
                                objectName: "geneticCode"
                                Accessible.name: "Genetic code"
                                model: dialog.geneticCodes.map(c => c.name)
                                currentIndex: Math.max(0, dialog.geneticCodes.findIndex(c => dialog.batch && c.id === dialog.batch.genetic_code))
                                onActivated: dialog.saveBatch()
                            }
                        }
                        ColumnLayout {
                            Label {
                                text: "Feature ends / completeness"
                            }
                            AppComboBox {
                                id: ends
                                Accessible.name: "Feature ends"
                                model: dialog.endPolicies.map(e => e[1])
                                currentIndex: dialog.indexOf(dialog.endPolicies, dialog.batch ? dialog.batch.ends || "auto" : "auto")
                                onActivated: dialog.saveBatch()
                            }
                        }
                    }
                    AppCheckBox {
                        objectName: "trimUnknown"
                        text: "Trim unknown bases (N, ?) at the ends of each sequence"
                        checked: !dialog.document || dialog.document.export.trim_terminal_unknown
                        onToggled: dialog.editRequested(JSON.stringify({
                            action: "set_export",
                            export: {
                                trim_terminal_unknown: checked
                            }
                        }))
                    }

                    AppButton {
                        objectName: "confirmFrames"
                        visible: dialog.coding
                        text: "Confirm detected frames"
                        enabled: !dialog.reviewing
                        onClicked: dialog.confirmDetectedRequested()
                    }
                    Label {
                        text: "Review and choose each reading frame in the Sequences table."
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }
                }
            }
            FocusScrollView {
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    Label {
                        text: "Submission authors"
                    }
                    Label {
                        text: "One author per line: Surname, Given Middle. Example: van der Meer, Anne M.\nNames are checked for format, not identity. Initials may use periods (M.R.); bare uppercase middle runs (MR) retain their legacy initials meaning. Full middle names such as Marie and hyphens are retained."
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }
                    TextArea {
                        id: authors
                        objectName: "submissionAuthors"
                        Layout.fillWidth: true
                        Accessible.name: "Submission authors"
                        text: dialog.document ? dialog.document.authors_text || "" : ""
                    }
                    Label {
                        text: "Submission title (exported as direct-submission description)"
                    }
                    TextField {
                        id: submissionTitle
                        objectName: "submissionTitle"
                        Layout.fillWidth: true
                        Accessible.name: "Submission title"
                        text: !dialog.document ? "" : (dialog.document.submission_details || dialog.document.publication).title
                    }
                    AppCheckBox {
                        id: publicationDiffers
                        objectName: "publicationDiffers"
                        text: "Publication details differ from submission details"
                        checked: !!dialog.document && !!dialog.document.publication_differs
                    }
                    Label {
                        visible: publicationDiffers.checked
                        text: "Publication authors — one per line: Surname, Given Middle"
                    }
                    TextArea {
                        id: publicationAuthors
                        objectName: "publicationAuthors"
                        visible: publicationDiffers.checked
                        Layout.fillWidth: true
                        Accessible.name: "Publication authors"
                        text: dialog.document ? dialog.document.publication_authors_text || dialog.document.authors_text || "" : ""
                    }
                    Label {
                        visible: publicationDiffers.checked
                        text: "Publication title"
                    }
                    TextField {
                        id: pubTitle
                        objectName: "publicationTitle"
                        visible: publicationDiffers.checked
                        Layout.fillWidth: true
                        Accessible.name: "Publication title"
                        text: dialog.document ? dialog.document.publication.title : ""
                    }
                    Label {
                        objectName: "authorsPreview"
                        readonly property var submissionParsed: dialog.parseAuthors(authors.text)
                        readonly property var publicationParsed: dialog.parseAuthors(publicationDiffers.checked ? publicationAuthors.text : authors.text)
                        text: submissionParsed.error || publicationParsed.error || "Export preview — submission: " + JSON.stringify(submissionParsed.export || []) + "\nPublication: " + JSON.stringify(publicationParsed.export || []) + "\nPublication title: " + (publicationDiffers.checked ? pubTitle.text : submissionTitle.text)
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }
                    AppButton {
                        id: savePublication
                        objectName: "savePublication"
                        text: "Save authors and titles"
                        onClicked: dialog.detailsRequested(JSON.stringify({
                            submission: {
                                title: submissionTitle.text,
                                authors: authors.text
                            },
                            publication: {
                                title: pubTitle.text,
                                authors: publicationAuthors.text
                            },
                            publication_differs: publicationDiffers.checked
                        }))
                    }
                    Label {
                        text: "The submitter's contact details are kept once for every submission."
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }
                    AppButton { objectName: "submissionContactDetails"; text: "Contact details…"; onClicked: dialog.contactDetailsRequested() }
                }
            }
            ColumnLayout {
                Label {
                    text: "Issues (" + dialog.review.issues.length + ") — select a record issue to inspect its sequence"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                ListView {
                    id: issueTable
                    objectName: "submissionIssues"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.minimumHeight: 100
                    clip: true
                    reuseItems: true
                    ScrollBar.vertical: ScrollBar {}
                    model: dialog.review.issues
                    delegate: ItemDelegate {
                        id: issue
                        required property var modelData
                        required property int index
                        objectName: "issue_" + index
                        width: issueTable.width
                        padding: 2
                        contentItem: Label {
                            text: (dialog.severityNames[issue.modelData.severity] || issue.modelData.severity) + " · " + (issue.modelData.record ? issue.modelData.record + ": " : "") + issue.modelData.message + (issue.modelData.source === "basecraft" ? "" : " [" + issue.modelData.source + " " + issue.modelData.code + "]")
                            color: issue.modelData.severity === "blocking" ? "#c0392b" : palette.windowText
                            wrapMode: Text.Wrap
                        }
                        Accessible.name: contentItem.text
                        enabled: issue.modelData.row !== null && issue.modelData.row !== undefined
                        onClicked: dialog.selectRow(issue.modelData.row)
                    }
                }

                FocusScrollView {
                    Layout.fillWidth: true
                    Layout.preferredHeight: Math.min(230, exportForm.implicitHeight)
                    contentWidth: availableWidth
                    ColumnLayout {
                        id: exportForm
                        width: parent.width
                        Label {
                            text: "Package and validation"
                            font.bold: true
                        }
                        Label {
                            objectName: "packageStatus"
                            text: !dialog.pkg || !dialog.pkg.exported ? "Not exported yet." : "Package " + dialog.pkg.directory + ": " + dialog.pkg.status + (dialog.pkg.validation ? ". table2asn " + dialog.pkg.validation.tool_version + " " + dialog.pkg.validation.status + " (" + dialog.pkg.validation.errors + " errors, " + dialog.pkg.validation.warnings + " warnings)" : ". Not validated for these files.")
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                        }
                        Label {
                            text: "Validation runs table2asn on this computer. A clean report means the files pass its checks; GenBank curators still review every submission."
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                        }
                        RowLayout {
                            spacing: 6
                            AppButton {
                                objectName: "exportPackage"
                                text: "Export package"
                                enabled: !!dialog.document && !!dialog.review.selection_confirmed && !dialog.validating && !dialog.reviewing
                                onClicked: dialog.exportRequested()
                            }
                            AppButton {
                                objectName: "validatePackage"
                                text: "Validate"
                                enabled: !!dialog.pkg && dialog.pkg.exported && !dialog.validating && !dialog.reviewing
                                onClicked: dialog.validateRequested()
                            }
                        }

                        AppMessageBar {
                            Layout.fillWidth: true
                            busy: dialog.validating
                            visible: busy
                            workingText: dialog.validationStage
                            cancelText: "Stop validation"
                            onCancelRequested: dialog.cancelValidationRequested()
                        }

                        AppCheckBox {
                            id: showReturned
                            text: "Record accessions returned by NCBI (after submission)"
                        }
                        ColumnLayout {
                            visible: showReturned.checked
                            Layout.fillWidth: true
                            TextArea {
                                id: accessionList
                                objectName: "accessionList"
                                Layout.fillWidth: true
                                placeholderText: "Paste the accession list NCBI sent: one sequence ID and accession per line"
                                Accessible.name: "Accession list from NCBI"
                                KeyNavigation.priority: KeyNavigation.BeforeItem
                                KeyNavigation.tab: matchAccessions
                            }
                            RowLayout {
                                spacing: 6
                                AppButton {
                                    id: matchAccessions
                                    objectName: "matchAccessions"
                                    text: "Match to records"
                                    enabled: accessionList.text.trim().length > 0
                                    onClicked: dialog.accessionsMatchRequested(accessionList.text)
                                }
                                AppButton {
                                    objectName: "recordAccessions"
                                    text: "Record " + (dialog.accessionPlan ? dialog.accessionPlan.accessions.length : 0) + " accessions"
                                    enabled: !!dialog.accessionPlan && dialog.accessionPlan.accessions.length > 0
                                    onClicked: {
                                        dialog.accessionsRecordRequested(JSON.stringify(dialog.accessionPlan.accessions));
                                        dialog.accessionPlan = null;
                                    }
                                }
                                AppButton {
                                    text: "Save accession table"
                                    enabled: !!dialog.document
                                    onClicked: dialog.accessionTableRequested()
                                }
                            }
                            Label {
                                visible: !!dialog.accessionPlan && dialog.accessionPlan.unmatched.length > 0
                                text: dialog.accessionPlan ? "Not matched: " + dialog.accessionPlan.unmatched.join("; ") : ""
                                wrapMode: Text.Wrap
                                Layout.fillWidth: true
                            }
                        }
                    }
                }
            }
        }
        RowLayout {
            AppButton {
                text: "History"
                onClicked: historyMenu.popup()
            }
            Label {
                text: "Edits save automatically. Undo: Ctrl+Z; redo: Ctrl+Shift+Z."
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            Menu {
                id: historyMenu
                MenuItem {
                    text: "Undo submission edit"
                    enabled: dialog.canUndo
                    onTriggered: dialog.undoRequested()
                }
                MenuItem {
                    text: "Redo submission edit"
                    enabled: dialog.canRedo
                    onTriggered: dialog.redoRequested()
                }
            }
        }
    }
    AppDialog {
        id: inclusionPreview
        implicitWidth: 1100
        title: "Review inclusion rule"
        standardButtons: Dialog.Cancel
        contentItem: ColumnLayout {
            implicitWidth: 1100
            implicitHeight: 480
            Label {
                text: !dialog.inclusionPlan ? "Preview invalidated by an edit; preview again." : dialog.inclusionPlan.matching + " matching · " + dialog.inclusionPlan.changed + " changes · " + dialog.inclusionPlan.preserved + " manual decisions preserved · " + dialog.inclusionPlan.unresolved + " unresolved · " + dialog.inclusionPlan.included + " included"
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            Label {
                text: "Identifier | extracted field | match | current → proposed | reason. Applying inclusion never verifies archive identity."
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            ListView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                reuseItems: true
                model: dialog.inclusionPlan ? dialog.inclusionPlan.rows : []
                ScrollBar.vertical: ScrollBar {}
                delegate: Label {
                    required property var modelData
                    width: ListView.view.width
                    text: modelData.record + " | " + (modelData.extracted === null ? "[missing]" : modelData.extracted === "" ? "[empty]" : modelData.extracted) + " | " + modelData.matched + " | " + modelData.current.decision + " → " + modelData.proposed.decision + " | " + modelData.reason
                    wrapMode: Text.Wrap
                }
            }
            AppButton {
                text: "Apply reviewed rule"
                objectName: "applyInclusion"
                enabled: !!dialog.inclusionPlan && !dialog.inclusionPlan.error
                onClicked: {
                    dialog.editRequested(JSON.stringify({
                        action: "apply_inclusion",
                        plan: dialog.inclusionPlan
                    }));
                    inclusionPreview.close();
                }
            }
        }
    }
}
