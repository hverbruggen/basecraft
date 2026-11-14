import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Qt.labs.qmlmodels
import Basecraft

// The window below the ribbon: the project's files, the open documents in
// tabs, and the active document's status line.
SplitView {
    id: area
    // The main window: the open document and the window's own functions.
    required property var app
    required property var clipboard
    required property var openCommand
    required property var menu
    required property var alnMenu
    required property var treeMenu
    required property var renameDialog
    required property var tipDialog
    required property var cladeEditor
    required property var editor
    required property var readOnlyCopy
    required property var operationWindow
    readonly property TabBar tabs: tabBar
    readonly property Repeater docs: documentSlots
    readonly property Item fileTree: filesView
    anchors.fill: parent
    anchors.topMargin: 4
    orientation: Qt.Horizontal

    // The project's files, from Qt's QFileSystemModel; collapsible.
    Rectangle {
        id: sidebar
        visible: app.sidebarOpen
        SplitView.preferredWidth: 200
        SplitView.minimumWidth: 120
        color: Qt.alpha(app.shell.foreground, 0.03)

        FileTreeModel {
            id: files
            rootDir: app.shell.project_dir
        }

        Label {
            id: projectName
            anchors { top: parent.top; left: parent.left; right: collapse.left; margins: 8 }
            text: app.shell.project_dir.length ? app.fileName(app.shell.project_dir) : "No project"
            font.bold: true
            elide: Text.ElideMiddle
        }
        AppIconButton {
            id: collapse
            anchors { verticalCenter: projectName.verticalCenter; right: parent.right; rightMargin: 2 }
            iconName: "left"; description: "Hide files"
            implicitWidth: 22
            implicitHeight: 22
            focusPolicy: Qt.StrongFocus
            ToolTip.text: "Hide files (Ctrl+B)"
            ToolTip.visible: hovered
            onClicked: app.sidebarOpen = false
        }

        TreeView {
            id: filesView
            anchors { top: projectName.bottom; topMargin: 6; left: parent.left; right: parent.right; bottom: parent.bottom }
            clip: true
            model: files
            rootIndex: files.rootIndex
            selectionModel: ItemSelectionModel {}
            delegate: FileTreeDelegate {
                currentPath: app.currentPath
                foreground: app.shell.foreground
                onOpenRequested: (path) => app.openPath(path)
            }
            ScrollBar.vertical: ScrollBar {}
        }
    }

    // Collapsed sidebar: a thin strip to bring it back.
    Rectangle {
        visible: !app.sidebarOpen
        SplitView.preferredWidth: 18
        SplitView.minimumWidth: 18
        SplitView.maximumWidth: 18
        color: Qt.alpha(app.shell.foreground, 0.03)
        AppIconButton {
            anchors { top: parent.top; horizontalCenter: parent.horizontalCenter; topMargin: 4 }
            iconName: "right"; description: "Show files"
            implicitWidth: 18
            implicitHeight: 22
            focusPolicy: Qt.StrongFocus
            ToolTip.text: "Show files (Ctrl+B)"
            ToolTip.visible: hovered
            onClicked: app.sidebarOpen = true
        }
    }

    // The open files: tabs, the active document, its status line.
    ColumnLayout {
        SplitView.fillWidth: true
        spacing: 0

        TabBar {
            id: tabBar
            objectName: "documentTabs"
            Layout.fillWidth: true
            visible: app.documentOpens.count > 0
            onCurrentIndexChanged: { if (!app.applyingOpenFocus) app.documentOpens.revoke_focus(); Qt.callLater(app.refreshCurrent) }
            Repeater {
                model: app.documentOpens
                AppTabButton {
                    required property int index
                    required property string path
                    required property bool dirty
                    onClicked: { app.documentOpens.revoke_focus(); app.focusOpenedDocument = true; Qt.callLater(app.refreshCurrent) }
                    Accessible.name: app.fileName(path) + (dirty ? ", unsaved changes" : "")
                    width: implicitWidth
                    focusPolicy: Qt.StrongFocus
                    contentItem: Row {
                        spacing: 6
                        Label {
                            // Long names shorten in the middle, keeping
                            // the extension; the path in the project is a
                            // hover away.
                            text: app.fileName(path) + (dirty ? " •" : "")
                            width: Math.min(implicitWidth, 180)
                            elide: Text.ElideMiddle
                            anchors.verticalCenter: parent.verticalCenter
                            ToolTip.text: app.shell.relative_path(path)
                            ToolTip.visible: tabHover.hovered
                            HoverHandler { id: tabHover }
                        }
                        AppIconButton {
                            iconName: "close"; description: "Close tab"
                            icon.width: 8
                            icon.height: 8
                            implicitWidth: 18
                            implicitHeight: 18
                            padding: 0
                            focusPolicy: Qt.StrongFocus
                            anchors.verticalCenter: parent.verticalCenter
                            onClicked: app.closeTab(index)
                        }
                    }
                }
            }
        }

        // Activity belongs to this document, beside its tabs and canvas.
        AppMessageBar {
            objectName: "documentActivity"
            Layout.fillWidth: true
            readonly property var controller: app.current
            busy: !!controller && (app.currentKind === "sequence" || app.alignmentOpen || app.treeOpen) && controller.busy
            visible: busy
            workingText: busy ? (app.alignmentOpen && controller.bootstrap_running
                ? controller.bootstrap_status : controller.message) || "Working" : ""
            cancellable: busy && app.alignmentOpen && controller.bootstrap_running && !controller.bootstrap_cancelling
            cancelText: "Cancel bootstrap"
            onCancelRequested: controller.cancel_bootstrap()
        }

        StackLayout {
            id: stack
            visible: app.documentOpens.count > 0
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: tabBar.currentIndex

            Repeater {
                id: documentSlots
                model: app.documentOpens
                onItemAdded: Qt.callLater(app.refreshCurrent)
                onItemRemoved: Qt.callLater(app.refreshCurrent)
                // Each file opens in the editor for what it holds.
                delegate: DelegateChooser {
                    role: "kind"
                    DelegateChoice {
                        roleValue: "canvas"
                        Item {
                            id: canvasSlot
                            required property string path
                            required property string openToken
                            required property int index
                            readonly property bool adoptedSlot: true
                            property bool ready: false
                            readonly property var documentView: canvasLoader.item
                            property var admittedController: null
                            property bool alive: true
                            Component.onDestruction: {
                                alive = false
                                app.documentOpens.retire(openToken)
                                if (admittedController) admittedController.destroy()
                            }
                            Component { id: sequenceOwner; GenomeController {} }
                            Component { id: alignmentOwner; AlignmentController {} }
                            Component { id: treeOwner; TreeController {} }
                            function construct(kind) {
                                if (!alive || admittedController) return
                                const factory = kind === "sequence" ? sequenceOwner : kind === "alignment" ? alignmentOwner : treeOwner
                                const owner = factory.createObject(canvasSlot)
                                if (!owner) { constructionFailed(); return }
                                admittedController = owner
                                if (!app.documentOpens.install(openToken, owner)) { constructionFailed(); return }
                                const values = {path: path, index: index, suppliedController: owner,
                                    overviewEnabled: app.overviewEnabled, notificationSink: app.provenance}
                                if (kind === "sequence") { values.showGc = app.showGc; values.defaultLayout = app.defaultLayout }
                                if (kind === "alignment") { values.fileUrls = app.shell; values.insertMode = app.alignmentInsert }
                                canvasLoader.setSource(kind === "sequence" ? "DocumentView.qml" : kind === "alignment" ? "AlignmentView.qml" : "PhyloView.qml", values)
                            }
                            function constructionFailed() {
                                if (!alive) return
                                const token = openToken
                                Qt.callLater(() => {
                                    const row = app.documentOpens.index_of(token)
                                    if (row >= 0) {
                                        app.provenance.notification(app.documentOpens.path_at(row), "Could not construct the document view", "fail")
                                        app.removeDocument(row)
                                    }
                                })
                            }
                            Loader {
                                id: canvasLoader
                                anchors.fill: parent
                                onStatusChanged: if (status === Loader.Error) canvasSlot.constructionFailed()
                            }
                            Binding { target: canvasLoader.item; property: "overviewEnabled"; value: app.overviewEnabled; when: canvasLoader.item !== null }
                            Binding { target: canvasLoader.item; property: "showGc"; value: app.showGc; when: canvasLoader.item !== null && canvasLoader.item.kind === "sequence" }
                            Binding { target: canvasLoader.item; property: "insertMode"; value: app.alignmentInsert; when: canvasLoader.item !== null && canvasLoader.item.kind === "alignment" }
                            Binding { target: canvasLoader.item; property: "index"; value: canvasSlot.index; when: canvasLoader.item !== null }
                            Binding { target: canvasLoader.item; property: "path"; value: canvasSlot.path; when: canvasLoader.item !== null }
                            Connections {
                                target: canvasLoader.item
                                ignoreUnknownSignals: true
                                function onInitializationCompleted() {
                                    if (!canvasSlot.alive || !app.documentOpens.acknowledge(canvasSlot.openToken, canvasSlot.admittedController)) return
                                    canvasSlot.ready = true
                                    app.documentOpens.activate(canvasSlot.openToken)
                                    app.refreshCurrent(false)
                                }
                                function onDirtyChanged() { app.documentOpens.set_dirty(canvasSlot.index, canvasLoader.item.dirty) }
                                function onActivate() { tabBar.currentIndex = canvasSlot.index }
                                function onOpenFile(path) { app.openPath(path) }
                                function onReviewTraces(paths, reference) { app.startTraceReview(paths, reference, canvasLoader.item) }
                                function onToggleInsert() { app.alignmentInsert = !app.alignmentInsert }
                                function onContextMenu(info) {
                                    if (canvasLoader.item.kind === "tree") treeMenu.dialog().showFor(canvasSlot.admittedController, info)
                                    else if (canvasLoader.item.kind === "alignment") { alnMenu.info = info; alnMenu.popup() }
                                    else { menu.info = info; menu.popup() }
                                }
                                function onCopyText(text) { clipboard.put(text) }
                                function onToggleSidebar() { app.sidebarOpen = !app.sidebarOpen }
                                function onRenameRow() { renameDialog.dialog().askRow() }
                                function onRenameTip() { tipDialog.dialog().ask() }
                                function onAnnotate() { cladeEditor.openSelected() }
                                function onEditFeature(id) { editor.openFeature(id) }
                                function onEditSelected() { editor.openSelected() }
                            }
                            AppMessageBar {
                                anchors { left: parent.left; right: parent.right; top: parent.top }
                                visible: !canvasSlot.ready
                                busy: true
                                workingText: "Opening " + app.fileName(canvasSlot.path)
                            }
                        }
                    }
                    DelegateChoice {
                        roleValue: "trace"
                        SangerView {
                            overviewEnabled: app.overviewEnabled
                            notificationSink: app.provenance
                            fileUrls: app.shell
                            onCopyText: (text) => clipboard.put(text)
                            onRedraft: (paths, reference, target, cutoff) => app.startTraceReview(paths, reference, target, cutoff, true)
                            onReadOnlyFolder: path => readOnlyCopy.dialog().offered = {file: path, review: true, row: index}
                            onDirtyChanged: app.documentOpens.set_dirty(index, dirty)
                            onWorkingChanged: app.documentOpens.set_working(index, working)
                        }
                    }
                    DelegateChoice {
                        roleValue: "alignment"
                        AlignmentView {
                            overviewEnabled: app.overviewEnabled
                            notificationSink: app.provenance
                            fileUrls: app.shell
                            insertMode: app.alignmentInsert
                            onActivate: tabBar.currentIndex = index
                            onReviewTraces: (paths, reference) => {
                                app.startTraceReview(paths, reference, app.documentAt(tabBar.currentIndex))
                            }
                            onToggleInsert: app.alignmentInsert = !app.alignmentInsert
                            onOpenFile: (path) => app.openPath(path)
                            onDirtyChanged: app.documentOpens.set_dirty(index, dirty)
                            onContextMenu: (info) => { alnMenu.info = info; alnMenu.popup() }
                            onCopyText: (text) => clipboard.put(text)
                            onToggleSidebar: app.sidebarOpen = !app.sidebarOpen
                            onRenameRow: renameDialog.dialog().askRow()
                        }
                    }
                    DelegateChoice {
                        roleValue: "tree"
                        // Not "TreeView": that's Qt's, used by the
                        // files sidebar.
                        PhyloView {
                            overviewEnabled: app.overviewEnabled
                            id: treeDocument
                            notificationSink: app.provenance
                            onOpenFile: (path) => app.openPath(path)
                            onDirtyChanged: app.documentOpens.set_dirty(index, dirty)
                            onContextMenu: (info) => treeMenu.dialog().showFor(treeDocument.controller, info)
                            onToggleSidebar: app.sidebarOpen = !app.sidebarOpen
                            onRenameTip: tipDialog.dialog().ask()
                            onAnnotate: cladeEditor.openSelected()
                        }
                    }
                    DelegateChoice {
                        roleValue: "delimitation"
                        DelimitationView {
                            overviewEnabled: app.overviewEnabled
                            notificationSink: app.provenance
                            onOpenFile: (path) => app.openPath(path)
                            onSmoothingRequested: path => operationWindow.dialog().showOperation(app.shell.project_dir.length ? app.shell.project_dir : app.shell.local_file_path(app.shell.parent_folder_url(path)), path, false, "basecraft.sanderson/date_tree@1")
                            onDirtyChanged: app.documentOpens.set_dirty(index, dirty)
                        }
                    }
                    DelegateChoice {
                        DocumentView {
                            id: sequenceDocument
                            overviewEnabled: app.overviewEnabled
                            notificationSink: app.provenance
                            showGc: app.showGc
                            defaultLayout: app.defaultLayout
                            onDirtyChanged: app.documentOpens.set_dirty(index, dirty)
                            onContextMenu: (info) => { menu.info = info; menu.popup() }
                            onEditFeature: (id) => editor.openFeature(id)
                            onEditSelected: editor.openSelected()
                            onInitializeNew: app.initializeSequence(sequenceDocument)
                            onCopyText: (text) => clipboard.put(text)
                            onToggleSidebar: app.sidebarOpen = !app.sidebarOpen
                        }
                    }
                }
            }
        }

        EmptyWorkspaceBrand {
            objectName: "emptyWorkspaceBrand"
            visible: app.documentOpens.count === 0 && app.shell.project_dir.length === 0
            Layout.fillWidth: true
            Layout.fillHeight: true
            backgroundColor: app.shell.background
            recentProjects: app.preferences.recent_projects
            onOpenProject: app.openProjectPicker()
            onOpenRecent: path => app.selectProject(path)
        }

        Item {
            visible: app.documentOpens.count === 0 && app.shell.project_dir.length > 0
            Layout.fillWidth: true
            Layout.fillHeight: true
            EmptyView {
                objectName: "emptyProjectDocuments"
                anchors.centerIn: parent
                width: Math.min(parent.width, 48 * font.pixelSize)
                message: "Open a sequence, alignment, chromatogram trace, tree, Sanger review, species delimitation workspace, specimen metadata table or GenBank submission. Choose a file on the left, or use Open to browse another folder."
                action: openCommand
                actionText: "Open"
            }
        }

        // The status line belongs to the open file, under it only.
        Rectangle {
            visible: app.documentOpens.count > 0
            Layout.fillWidth: true
            implicitHeight: 20
            color: Qt.alpha(app.shell.foreground, 0.06)
            RowLayout {
                anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
                spacing: 12
                Label {
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                    text: app.current ? app.current.status : ""
                }
                // A tree's linked alignment, such as "No linked alignment".
                Label {
                    objectName: "treeAlignmentStatus"
                    readonly property string linkStatus: app.treeOpen && app.current ? (app.current.alignment_link_status || "") : ""
                    visible: linkStatus.length > 0
                    text: linkStatus
                    Layout.maximumWidth: 360
                    elide: Text.ElideMiddle
                }
                Label {
                    readonly property string hoverText: app.alignmentDocument ? app.alignmentDocument.hoverStatus
                        : app.traceDocument ? app.traceDocument.hoverStatus : ""
                    visible: hoverText.length > 0
                    text: hoverText
                    Layout.maximumWidth: parent.width * 0.6
                    elide: Text.ElideLeft
                }
            }
        }
    }
}
