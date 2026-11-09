import QtQuick
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Controls.Basic
import Basecraft

// One open alignment: its controller, the GPU view, and pointer and key
// handling. Window-level actions (menus, dialogs, the sidebar) are asked
// for through signals, as in DocumentView.
FocusScope {
    id: doc
    property alias findBar: findBar
    function openFind() { findBar.openFind() }
    FindBar {
        id: findBar
        inputHint: "Taxon name or motif"
        sequenceSearch: true
        nameSearch: true
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        controller: ctl
        documentFocusItem: view
        activeDocument: doc.visible
    }

    property bool overviewEnabled: true
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
    readonly property bool dirty: ctl.dirty
    readonly property string kind: "alignment"
    readonly property string hoverStatus: visible ? mouse.hoverText : ""
    // Typing inserts rather than overwrites; set by the window.
    property bool insertMode: false

    property bool ribbonInitialized: false
    function activeFolder() { const p = ctl.file_path.length ? ctl.file_path : doc.path; return fileUrls.parent_folder_url(p) }
    signal contextMenu(var info)
    signal copyText(string text)
    signal toggleSidebar
    signal renameRow
    signal toggleInsert
    // A tool wrote a new file, to open in a tab.
    signal openFile(string path)

    signal activate
    signal reviewTraces(var paths, string reference)
    function localPath(url) { return fileUrls.local_file_path(url) }
    function showAdditionChoices() { if (activeAdditionDialog) activeAdditionDialog.showChoices() }
    property var activeAdditionDialog: null
    function addText() { if (ctl.busy) return; activeAdditionDialog = textSequenceDialog; textSequenceDialog.prepare("") }
    function addFasta() { if (!ctl.busy) fastaDialog.open() }
    function addTraces() { traceDialog.open() }
    function addReviewed(sequence, name, mafft = true, evidence = "") { if (ctl.busy) return; doc.activate(); activeAdditionDialog = reviewedSequenceDialog; reviewedSequenceDialog.prepare(sequence, name, mafft ? 1 : 0, evidence) }
    function prepareFasta(text) { if (ctl.busy) return; activeAdditionDialog = fastaSequenceDialog; fastaSequenceDialog.prepare(text) }

    AlignmentController { id: ctl }
    FileDialog {
        id: fastaDialog
        currentFolder: doc.activeFolder()
        onVisibleChanged: if (visible) currentFolder = doc.activeFolder()
        title: "Add all sequences from FASTA"
        nameFilters: ["FASTA (*.fa *.fasta *.fas *.fna *.afa)", "All files (*)"]
        onAccepted: {
            const text = ctl.read_sequence_file(doc.localPath(selectedFile))
            if (text.length) doc.prepareFasta(text)
        }
    }
    FileDialog {
        id: traceDialog
        currentFolder: doc.activeFolder()
        onVisibleChanged: if (visible) currentFolder = doc.activeFolder()
        title: "Choose 1–3 reads from the same specimen and amplicon"
        fileMode: FileDialog.OpenFiles
        nameFilters: ["Chromatograms (*.ab1 *.abi)", "All files (*)"]
        onAccepted: {
            if (selectedFiles.length < 1 || selectedFiles.length > 3) { tooManyReads.open(); return }
            const reference = ctl.review_snapshot()
            if (reference.length) doc.reviewTraces(selectedFiles.map(url => doc.localPath(url)), reference)
        }
    }
    AppFormDialog {
        id: tooManyReads
        title: "Select one to three reads"
        implicitWidth: 480
        modal: true
        standardButtons: Dialog.Ok
        Label { Layout.fillWidth: true; text: "Review one specimen and amplicon at a time, using up to three reads."; wrapMode: Text.Wrap }
    }
    AddSequenceDialog {
        id: textSequenceDialog
        controller: ctl
        inputKind: "text"
        onInputError: (message) => additionError.show(message)
    }
    AddSequenceDialog {
        id: fastaSequenceDialog
        controller: ctl
        inputKind: "fasta"
        onInputError: (message) => additionError.show(message)
    }
    AddSequenceDialog {
        id: reviewedSequenceDialog
        controller: ctl
        inputKind: "reviewed"
        onInputError: (message) => additionError.show(message)
    }
    AdditionErrorDialog {
        id: additionError
        onCopyMessage: (text) => doc.copyText(text)
    }
    AdditionGapsDialog {
        id: insertionDialog
        additionDialog: doc.activeAdditionDialog
    }

    Binding { target: ctl; property: "insert_mode"; value: doc.insertMode }
    Connections {
        target: ctl
        function onFileCreated(path) { doc.openFile(path) }
        function onSequencesAdded() { if (doc.activeAdditionDialog) doc.activeAdditionDialog.close() }
        function onAdditionFailed(message) { additionError.show(message) }
        function onAdditionNeedsGaps(count) { insertionDialog.count = count; insertionDialog.open() }
    }

    // Open once the view has its real size.
    Component.onCompleted: Qt.callLater(() => {
        ctl.resize(view.width, view.height)
        ctl.open_file(doc.path)
    })

    AppCanvas {
        id: view
        accessibleTitle: "Alignment editor"
        accessibleSummary: ctl.status
        anchors.fill: parent
        anchors.bottomMargin: findBar.height + (hbar.visible ? hbar.height : 0) + (overview.visible ? overview.height : 0)
        clearColor: ctl.background
        fontFamily: ctl.mono_font
        instances: ctl.frame
        focus: true
        onWidthChanged: ctl.resize(width, height)
        onHeightChanged: ctl.resize(width, height)

        MouseArea {
            id: mouse
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
            hoverEnabled: true
            property string hoverText: ""
            property real lastX: 0
            property real lastY: 0
            onPressed: (mouse) => {
                view.forceActiveFocus()
                hoverText = ""
                if (mouse.button === Qt.RightButton)
                    doc.contextMenu(JSON.parse(ctl.context_press(mouse.x, mouse.y)))
                else if (mouse.button === Qt.MiddleButton)
                    ctl.pan_press(mouse.x, mouse.y)
                else
                    ctl.press(mouse.x, mouse.y, mouse.modifiers)
            }
            onPositionChanged: (mouse) => {
                lastX = mouse.x; lastY = mouse.y
                if (mouse.buttons & (Qt.LeftButton | Qt.MiddleButton)) {
                    ctl.drag(mouse.x, mouse.y)
                    edgeTimer.running = (mouse.buttons & Qt.LeftButton)
                        && (mouse.x < 24 || mouse.x > view.width - 24 || mouse.y < 24 || mouse.y > view.height - 24)
                } else {
                    hoverText = ctl.hover(mouse.x, mouse.y)
                }
            }
            onExited: hoverText = ""
            onReleased: { edgeTimer.running = false; ctl.release() }
            Timer {
                id: edgeTimer
                interval: 16
                repeat: true
                onTriggered: running = ctl.edge_scroll(parent.lastX, parent.lastY) && parent.pressed
            }
            onDoubleClicked: (mouse) => ctl.double_click(mouse.x, mouse.y)
            onWheel: (wheel) => ctl.wheel(wheel.x, wheel.y, wheel.angleDelta.y, wheel.angleDelta.x, wheel.modifiers)
        }

        ScrollBar {
            id: hbar
            orientation: Qt.Horizontal
            // Under the residues only, not the names.
            anchors { left: parent.left; leftMargin: ctl.names_width; right: parent.right; rightMargin: vbar.width; top: parent.bottom }
            policy: ScrollBar.AlwaysOn
            visible: ctl.hscroll_size < 0.999
            size: ctl.hscroll_size
            minimumSize: 0.04
            contentItem: Rectangle {
                implicitHeight: 10
                radius: 5
                color: Qt.alpha(ctl.foreground, hbar.pressed ? 0.65 : 0.4)
            }
            background: Rectangle { color: Qt.alpha(ctl.foreground, 0.06) }
            onPositionChanged: if (pressed) ctl.hscroll_to(position)
        }
        ScrollBar {
            id: vbar
            orientation: Qt.Vertical
            anchors { top: parent.top; right: parent.right; bottom: parent.bottom }
            policy: ScrollBar.AlwaysOn
            visible: ctl.vscroll_size < 0.999
            width: visible ? implicitWidth : 0
            size: ctl.vscroll_size
            minimumSize: 0.04
            contentItem: Rectangle {
                implicitWidth: 10
                radius: 5
                color: Qt.alpha(ctl.foreground, vbar.pressed ? 0.65 : 0.4)
            }
            background: Rectangle { color: Qt.alpha(ctl.foreground, 0.06) }
            onPositionChanged: if (pressed) ctl.vscroll_to(position)
        }
        // Follow the controller except while the user drags a bar.
        Binding { target: vbar; property: "position"; value: ctl.vscroll_pos; when: !vbar.pressed; restoreMode: Binding.RestoreNone }
        Binding { target: hbar; property: "position"; value: ctl.hscroll_pos; when: !hbar.pressed; restoreMode: Binding.RestoreNone }

        PinchHandler {
            target: null
            property real last: 1
            onActiveChanged: last = 1
            onActiveScaleChanged: {
                ctl.zoom_by(centroid.position.x, centroid.position.y, activeScale / last)
                last = activeScale
            }
        }

        Keys.onPressed: (event) => {
            if (event.key === Qt.Key_Escape && findBar.opened) { findBar.closeFind(); event.accepted = true; return }
            if (!view.renderingAvailable) { event.accepted = false; return }
            const ctrl = event.modifiers & Qt.ControlModifier
            if (ctrl && event.key === Qt.Key_B) { doc.toggleSidebar(); event.accepted = true; return }
            if (ctrl && event.key === Qt.Key_C) {
                const t = ctl.selection_text()
                if (t.length > 0) doc.copyText(t)
                event.accepted = true; return
            }
            if (event.key === Qt.Key_F2) { doc.renameRow(); event.accepted = true; return }
            if (event.key === Qt.Key_Insert) { doc.toggleInsert(); event.accepted = true; return }
            event.accepted = ctl.busy || ctl.key(event.key, event.text, event.modifiers)
        }
    }
    DocumentOverview {
        id: overview
        anchors { left: parent.left; right: parent.right; bottom: findBar.top }
        shown: doc.overviewEnabled
        miniature: ctl.overview_content
        matchMarks: JSON.parse(ctl.overview_matches || "[]")
        foreground: ctl.foreground; backgroundColor: ctl.background; accent: ctl.accent
        documentItem: view
        viewportX: ctl.hscroll_pos; viewportY: ctl.vscroll_pos
        viewportWidth: ctl.hscroll_size; viewportHeight: ctl.vscroll_size
        onPanRequested: (x, y) => { ctl.hscroll_to(x); ctl.vscroll_to(y) }
    }
}
