import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppDialog {
    id: dialog
    objectName: "documentRecordDialog"
    modal: false
    implicitWidth: 900
    implicitHeight: 660
    standardButtons: Dialog.Close
    title: "Document record"
    property var controller: null
    property var service: null
    property string documentPath: ""
    property bool savedRevision: false
    property var report: ({})
    property string token: ""
    property int page: 0
    property string selectedId: ""
    property var drafts: ({})
    property string draftKey: ""
    property string savedDraftText: ""
    property string draftText: ""
    property string draftRevision: ""
    readonly property string documentName: report.document || ""
    readonly property var historyRows: (report.history || []).filter(row => allEvents.checked || row.contributing || row.status === "Source" || row.status === "Saved checkpoint")
    signal copyText(string text)
    signal exportReport(string snapshot)
    signal saveDraft(string text, string revision)
    signal verify()
    signal packageRecord()
    signal openRecordFolder()
    initialFocusItem: pages

    function rememberDraft() {
        if (draftKey.length) drafts[draftKey] = { text: draftText, revision: draftRevision, savedText: savedDraftText }
    }
    function unsavedDrafts() {
        rememberDraft()
        return Object.keys(drafts).filter(key => drafts[key].text !== drafts[key].savedText)
    }
    function markDraftSaved(key, text) {
        if (drafts[key]) drafts[key].savedText = text
        if (draftKey === key) savedDraftText = text
    }
    function refreshReport() {
        if (!controller) { rememberDraft(); report = ({}); token = ""; return }
        const nextToken = controller.record_revision() + ":" + savedRevision
        if (nextToken === token && report.document) return
        rememberDraft()
        token = nextToken
        const next = JSON.parse(savedRevision && service ? service.saved_report(documentPath) : controller.document_record())
        const nextKey = documentPath || next.document || ""
        const changedDocument = nextKey !== draftKey
        report = next
        if (changedDocument) {
            selectedId = ""
            const draft = drafts[nextKey]
            draftKey = nextKey
            savedDraftText = draft ? draft.savedText || "" : ""
            draftText = draft ? draft.text : ""
            draftRevision = draft ? draft.revision : ""
        }
    }
    function showPage(index) { page = index; token = ""; refreshReport(); open() }
    function showEvidence(id) { selectedId = id; allEvents.checked = true; page = 0 }
    onSavedRevisionChanged: { token = ""; if (visible) refreshReport() }
    onControllerChanged: { token = ""; if (visible) refreshReport() }
    onClosed: rememberDraft()
    // Every edit or finished run redraws the document or rewrites the workspace.
    readonly property var redrawn: controller ? [controller.frame, controller.workspace_json] : []
    onRedrawnChanged: if (visible) refreshReport()

    contentItem: ColumnLayout {
        spacing: 8
        Label { Layout.fillWidth: true; text: dialog.documentName; elide: Text.ElideMiddle; font.bold: true }
        Label {
            Layout.fillWidth: true; wrapMode: Text.Wrap
            text: dialog.report.error || (dialog.savedRevision ? "Last saved revision" : dialog.report.unsaved ? "Current document — includes unsaved changes" : "Current document")
            Accessible.role: dialog.report.error ? Accessible.AlertMessage : Accessible.StaticText
        }
        Label { Layout.fillWidth: true; text: "Revision: " + (dialog.report.revision || "Unavailable"); elide: Text.ElideMiddle }
        AppCheckBox {
            text: "Show last saved revision"
            checked: dialog.savedRevision
            enabled: !!dialog.service
            onToggled: dialog.savedRevision = checked
        }
        TabBar {
            id: pages
            objectName: "recordTabs"
            implicitWidth: 0
            Layout.minimumWidth: 0
            Layout.maximumWidth: dialog.availableWidth
            Layout.fillWidth: true
            currentIndex: dialog.page
            onCurrentIndexChanged: dialog.page = currentIndex
            AppTabButton { width: pages.width / 4; text: "Provenance" }
            AppTabButton { width: pages.width / 4; text: "Methods" }
            AppTabButton { width: pages.width / 4; text: "Authored draft" }
            AppTabButton { width: pages.width / 4; text: "Raw record" }
        }
        StackLayout {
            Layout.minimumWidth: 0
            Layout.maximumWidth: dialog.availableWidth
            currentIndex: dialog.page
            Layout.fillWidth: true
            Layout.fillHeight: true
            ColumnLayout {
                AppCheckBox { id: allEvents; text: "All recorded events (including undo and related attempts)" }
                Label { text: (dialog.report.warnings || []).join("\n"); wrapMode: Text.Wrap; Layout.fillWidth: true; visible: text.length > 0 }
                SplitView {
                    Layout.fillWidth: true; Layout.fillHeight: true
                    orientation: width > 650 ? Qt.Horizontal : Qt.Vertical
                    ScrollView {
                        SplitView.preferredWidth: 360; SplitView.preferredHeight: 180
                        ListView {
                            id: historyList
                            clip: true
                            model: dialog.historyRows
                            delegate: ItemDelegate {
                                required property var modelData
                                width: ListView.view.width
                                highlighted: dialog.selectedId === modelData.id
                                text: modelData.summary + "\n" + modelData.status
                                Accessible.name: text
                                onClicked: dialog.selectedId = modelData.id
                            }
                            EmptyView { anchors.centerIn: parent; visible: historyList.count === 0; message: "No recorded changes for this revision. Document edits and analyses appear here when recorded; enable All recorded events to include related attempts."; width: parent.width }
                        }
                    }
                    ScrollView {
                        SplitView.fillWidth: true; SplitView.fillHeight: true
                        TextArea {
                            readOnly: true; selectByMouse: true; wrapMode: TextEdit.Wrap
                            Accessible.name: "History entry details"
                            text: {
                                const row = (dialog.report.history || []).find(r => r.id === dialog.selectedId)
                                return row ? row.summary + "\n\n" + row.details : "Select a history entry to inspect its evidence and exact settings."
                            }
                        }
                    }
                }
            }
            ScrollView {
                clip: true
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    TextArea {
                        Layout.fillWidth: true
                        text: dialog.report.methods || "No methods are available."
                        readOnly: true; selectByMouse: true; wrapMode: TextEdit.Wrap
                        Accessible.name: "Methods for the current document"
                    }
                    Label { text: "Supporting steps"; font.bold: true }
                    Repeater {
                        model: dialog.report.paragraphs || []
                        delegate: AppButton {
                            required property var modelData
                            required property int index
                            text: "Paragraph " + (index + 1) + " — show evidence and parameters"
                            onClicked: dialog.showEvidence(modelData.activity_ids[0])
                        }
                    }
                }
            }
            ColumnLayout {
                Label {
                    text: dialog.draftRevision && dialog.draftRevision !== dialog.report.revision ? "This draft describes an earlier revision. Review the current Methods before updating it." : "Your text is kept separate from generated Methods. Copy or save it before closing the application."
                    Layout.fillWidth: true; wrapMode: Text.Wrap
                }
                AppButton {
                    text: "Start from current Methods"
                    enabled: !dialog.draftText.length
                    onClicked: { dialog.draftText = dialog.report.methods || ""; dialog.draftRevision = dialog.report.revision || "" }
                }
                ScrollView {
                    Layout.fillWidth: true; Layout.fillHeight: true
                    TextArea {
                        text: dialog.draftText; selectByMouse: true; wrapMode: TextEdit.Wrap
                        Accessible.name: "Authored methods draft"
                        onTextChanged: { dialog.draftText = text; dialog.rememberDraft() }
                    }
                }
                RowLayout {
                    AppButton { text: "Copy draft"; onClicked: dialog.copyText(dialog.draftText) }
                    AppButton { text: "Save draft…"; enabled: dialog.draftText.length > 0; onClicked: dialog.saveDraft(dialog.draftText, dialog.draftRevision) }
                }
            }
            ScrollView {
                TextArea { text: dialog.report.raw || ""; readOnly: true; selectByMouse: true; wrapMode: TextEdit.Wrap; Accessible.name: "Raw document record" }
            }
        }
        Flow {
            Layout.fillWidth: true
            Layout.preferredHeight: childrenRect.height
            spacing: 6
            AppButton { objectName: "copyRecordText"; text: "Copy " + (dialog.page === 1 ? "methods" : "history"); onClicked: dialog.copyText(dialog.page === 1 ? dialog.report.methods || "" : dialog.report.history_text || "") }
            AppButton { objectName: "openMethodsProvenance"; text: "Open provenance"; visible: dialog.page === 1; onClicked: dialog.page = 0 }
            AppButton { text: "Export report…"; enabled: !!dialog.report.revision; onClicked: dialog.exportReport(JSON.stringify(dialog.report)) }
            AppButton { text: "Check saved files"; onClicked: dialog.verify() }
            AppButton { text: "Export reproduction package…"; onClicked: dialog.packageRecord() }
            AppButton { text: "Record folder"; onClicked: dialog.openRecordFolder() }
        }
    }
}
