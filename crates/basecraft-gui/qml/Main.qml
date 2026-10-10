import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import Basecraft

ApplicationWindow {
    id: window
    width: 1200
    height: 760
    visible: true
    title: current ? fileName(currentPath) + " — Basecraft" : "Basecraft"
    color: shell.background
    font.family: shell.mono_font
    font.pixelSize: 11

    palette.window: shell.background
    palette.base: Qt.lighter(shell.background, 1.04)
    palette.button: Qt.darker(shell.background, 1.05)
    palette.text: shell.foreground
    palette.windowText: shell.foreground
    palette.buttonText: shell.foreground
    palette.highlight: shell.accent
    palette.highlightedText: shell.background

    // Theme, font and project folder, whether or not a file is open.
    GenomeController { id: shell }

    // Open files, one tab each.
    ListModel { id: openFiles }

    // The active tab's controller and view, or null.
    property var current: null
    property var currentView: null
    property string currentPath: ""
    property bool sidebarOpen: true

    function fileName(path) { return path.split("/").pop() }

    function refreshCurrent() {
        const d = docs.itemAt(tabs.currentIndex)
        current = d ? d.controller : null
        currentView = d ? d.view : null
        currentPath = d ? d.path : ""
        if (currentView) currentView.forceActiveFocus()
    }

    // Open a file in a tab, or switch to its tab if it's already open.
    function openPath(path) {
        for (let i = 0; i < openFiles.count; i++) {
            if (openFiles.get(i).path === path) { tabs.currentIndex = i; return }
        }
        if (!shell.project_dir.length) shell.use_project_of(path)
        openFiles.append({ path: path, dirty: false })
        tabs.currentIndex = openFiles.count - 1
    }

    function closeTab(i) {
        const d = docs.itemAt(i)
        if (d && d.controller.dirty) { confirmClose.index = i; confirmClose.open(); return }
        openFiles.remove(i)
        Qt.callLater(refreshCurrent)
    }

    // Clipboard access goes through a hidden text edit.
    TextEdit {
        id: clipboard
        visible: false
        function copy(text) { clipboard.text = text; clipboard.selectAll(); clipboard.copy() }
    }

    component Tool: ToolButton {
        focusPolicy: Qt.NoFocus
        enabled: window.current !== null
    }

    header: ToolBar {
        background: Rectangle { color: Qt.alpha(shell.foreground, 0.04) }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 6
            spacing: 2
            Tool { text: "Files"; enabled: true; checkable: true; checked: window.sidebarOpen; ToolTip.text: "Show or hide the project's files (Ctrl+B)"; ToolTip.visible: hovered; onClicked: window.sidebarOpen = !window.sidebarOpen }
            ToolSeparator {}
            Tool { text: "Wrapped"; checkable: true; checked: window.current?.mode === "wrapped"; onClicked: window.current.switch_view("wrapped") }
            Tool { text: "Linear"; checkable: true; checked: window.current?.mode === "linear"; onClicked: window.current.switch_view("linear") }
            Tool { text: "Circular"; checkable: true; checked: window.current?.mode === "circular"; onClicked: window.current.switch_view("circular") }
            ToolSeparator {}
            Tool { text: "−"; ToolTip.text: "Zoom out (−)"; ToolTip.visible: hovered; onClicked: window.current.zoom_out() }
            Tool { text: "+"; ToolTip.text: "Zoom in (+, double-click)"; ToolTip.visible: hovered; onClicked: window.current.zoom_in() }
            Tool { text: "Fit"; ToolTip.text: "Whole sequence (Ctrl+0)"; ToolTip.visible: hovered; onClicked: window.current.fit() }
            Tool { text: "Zoom to selection"; onClicked: window.current.zoom_to_selection() }
            Label {
                text: window.current ? window.current.zoom_text : ""
                leftPadding: 8
                rightPadding: 4
                opacity: 0.75
            }
            ToolSeparator {}
            Tool { text: "Add annotation…"; ToolTip.text: "Annotate the selection (Ctrl+N)"; ToolTip.visible: hovered; onClicked: editor.openNew() }
            Tool { text: "Edit annotation…"; ToolTip.text: "Edit the selected annotation (Enter)"; ToolTip.visible: hovered; onClicked: editor.openSelected() }
            ToolSeparator {}
            Tool { text: "Undo"; onClicked: window.current.undo() }
            Tool { text: "Redo"; onClicked: window.current.redo() }
            Tool { text: window.current?.dirty ? "Save •" : "Save"; onClicked: window.current.save() }
            Label {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignRight
                rightPadding: 10
                elide: Text.ElideLeft
                text: window.current ? window.current.message : ""
                color: shell.accent
            }
        }
    }

    SplitView {
        anchors.fill: parent
        anchors.topMargin: 4
        orientation: Qt.Horizontal

        // The project's files, from Qt's QFileSystemModel; collapsible.
        Rectangle {
            id: sidebar
            visible: window.sidebarOpen
            SplitView.preferredWidth: 200
            SplitView.minimumWidth: 120
            color: Qt.alpha(shell.foreground, 0.03)

            FileTreeModel {
                id: files
                rootDir: shell.project_dir
            }

            Label {
                id: projectName
                anchors { top: parent.top; left: parent.left; right: collapse.left; margins: 8 }
                text: shell.project_dir.length ? window.fileName(shell.project_dir) : "No project"
                font.pixelSize: 10
                font.bold: true
                elide: Text.ElideMiddle
            }
            ToolButton {
                id: collapse
                anchors { verticalCenter: projectName.verticalCenter; right: parent.right; rightMargin: 2 }
                text: "◀"
                font.pixelSize: 10
                implicitWidth: 22
                implicitHeight: 22
                focusPolicy: Qt.NoFocus
                ToolTip.text: "Hide files (Ctrl+B)"
                ToolTip.visible: hovered
                onClicked: window.sidebarOpen = false
            }

            TreeView {
                id: tree
                anchors { top: projectName.bottom; topMargin: 6; left: parent.left; right: parent.right; bottom: parent.bottom }
                clip: true
                model: files
                rootIndex: files.rootIndex
                selectionModel: ItemSelectionModel {}
                delegate: TreeViewDelegate {
                    implicitWidth: tree.width
                    readonly property string path: files.pathAt(treeView.index(row, column))
                    highlighted: path === window.currentPath
                    font.pixelSize: 10
                    implicitHeight: 17
                    topPadding: 0
                    bottomPadding: 0
                    onClicked: {
                        if (files.isDirAt(treeView.index(row, column))) treeView.toggleExpanded(row)
                        else window.openPath(path)
                    }
                }
                ScrollBar.vertical: ScrollBar {}
            }
        }

        // Collapsed sidebar: a thin strip to bring it back.
        Rectangle {
            visible: !window.sidebarOpen
            SplitView.preferredWidth: 18
            SplitView.minimumWidth: 18
            SplitView.maximumWidth: 18
            color: Qt.alpha(shell.foreground, 0.03)
            ToolButton {
                anchors { top: parent.top; horizontalCenter: parent.horizontalCenter; topMargin: 4 }
                text: "▶"
                font.pixelSize: 10
                implicitWidth: 18
                implicitHeight: 22
                focusPolicy: Qt.NoFocus
                ToolTip.text: "Show files (Ctrl+B)"
                ToolTip.visible: hovered
                onClicked: window.sidebarOpen = true
            }
        }

        // The open files: tabs, the active document, its status line.
        ColumnLayout {
            SplitView.fillWidth: true
            spacing: 0

            TabBar {
                id: tabs
                Layout.fillWidth: true
                visible: openFiles.count > 0
                onCurrentIndexChanged: Qt.callLater(window.refreshCurrent)
                Repeater {
                    model: openFiles
                    TabButton {
                        required property int index
                        required property string path
                        required property bool dirty
                        width: implicitWidth
                        focusPolicy: Qt.NoFocus
                        contentItem: Row {
                            spacing: 6
                            Label {
                                // Long names shorten in the middle, keeping
                                // the extension; the full name is a hover away.
                                text: window.fileName(path) + (dirty ? " •" : "")
                                font.pixelSize: 10
                                width: Math.min(implicitWidth, 180)
                                elide: Text.ElideMiddle
                                anchors.verticalCenter: parent.verticalCenter
                                ToolTip.text: path
                                ToolTip.visible: tabHover.hovered && truncated
                                HoverHandler { id: tabHover }
                            }
                            ToolButton {
                                text: "×"
                                implicitWidth: 18
                                implicitHeight: 18
                                padding: 0
                                focusPolicy: Qt.NoFocus
                                anchors.verticalCenter: parent.verticalCenter
                                onClicked: window.closeTab(index)
                            }
                        }
                    }
                }
            }

            StackLayout {
                id: stack
                Layout.fillWidth: true
                Layout.fillHeight: true
                currentIndex: tabs.currentIndex

                Repeater {
                    id: docs
                    model: openFiles
                    onItemAdded: Qt.callLater(window.refreshCurrent)
                    onItemRemoved: Qt.callLater(window.refreshCurrent)
                    DocumentView {
                        onContextMenu: (info) => { menu.info = info; menu.popup() }
                        onEditFeature: (id) => editor.openFeature(id)
                        onEditSelected: editor.openSelected()
                        onNewAnnotation: editor.openNew()
                        onCopyText: (text) => clipboard.copy(text)
                        onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                    }
                }
            }

            Label {
                visible: openFiles.count === 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                text: "Open a sequence from the files on the left"
                opacity: 0.6
            }

            // The status line belongs to the open file, under it only.
            Rectangle {
                visible: openFiles.count > 0
                Layout.fillWidth: true
                implicitHeight: 20
                color: Qt.alpha(shell.foreground, 0.06)
                Label {
                    anchors { verticalCenter: parent.verticalCenter; left: parent.left; leftMargin: 10; right: parent.right; rightMargin: 10 }
                    elide: Text.ElideRight
                    font.pixelSize: 10
                    text: window.current ? window.current.status : ""
                }
            }
        }
    }

    // Only what applies where you clicked: an annotation, or the selection.
    // Global actions (zoom, whole sequence) live in the toolbar.
    Menu {
        id: menu
        property var info: ({ feature: -1, label: "", featureLength: 0, selection: false, length: 0 })
        readonly property bool onAnnotation: info.feature >= 0
        component Item: MenuItem { height: visible ? implicitHeight : 0 }

        Item { text: "Edit “" + menu.info.label + "”…"; visible: menu.onAnnotation; onTriggered: editor.openFeature(menu.info.feature) }
        Item { text: "Delete “" + menu.info.label + "”"; visible: menu.onAnnotation; onTriggered: window.current.delete_feature(menu.info.feature) }
        Item {
            text: "Copy annotation sequence (" + menu.info.featureLength + " bp)"
            visible: menu.onAnnotation
            onTriggered: clipboard.copy(window.current.feature_sequence(menu.info.feature))
        }
        Item { text: "Extract as new sequence"; visible: menu.onAnnotation; onTriggered: window.current.extract_feature(menu.info.feature) }
        Item { text: "Zoom to annotation"; visible: menu.onAnnotation; onTriggered: window.current.zoom_to_feature(menu.info.feature) }

        Item { text: "Add annotation…"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: editor.openNew() }
        Item {
            text: "Copy sequence (" + menu.info.length + " bp)"
            visible: !menu.onAnnotation
            enabled: menu.info.selection
            onTriggered: clipboard.copy(window.current.selection_text())
        }
        Item { text: "Extract as new sequence"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: window.current.extract_selection() }
        Item { text: "Zoom to selection"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: window.current.zoom_to_selection() }
    }

    Dialog {
        id: confirmClose
        property int index: -1
        title: "Save changes?"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Save | Dialog.Discard | Dialog.Cancel
        Label { text: "This file has unsaved changes." }
        onAccepted: {
            docs.itemAt(index).controller.save()
            openFiles.remove(index)
            Qt.callLater(window.refreshCurrent)
        }
        onDiscarded: {
            close()
            openFiles.remove(index)
            Qt.callLater(window.refreshCurrent)
        }
    }

    FeatureEditor {
        id: editor
        controller: window.current
        onClosed: if (window.currentView) window.currentView.forceActiveFocus()
    }

    Timer {
        interval: 1500
        running: true
        repeat: true
        onTriggered: {
            shell.check_theme()
            for (let i = 0; i < docs.count; i++) {
                const d = docs.itemAt(i)
                if (d) { d.controller.check_theme(); d.controller.flush() }
            }
        }
    }

    function arg(name) {
        const args = Qt.application.arguments
        const i = args.indexOf(name)
        return i >= 0 ? args[i + 1] : undefined
    }

    Component.onCompleted: {
        const args = Qt.application.arguments
        for (let i = 1; i < args.length; i++) {
            if (args[i].startsWith("--")) { i++; continue }
            shell.use_project_of(args[i])
            openPath(args[i])
        }
    }

    // Scripted runs, on the active tab: --goto START,END  --mode wrapped|linear|circular
    // --type TEXT  --select X0,X1[,Y]  --menu X,Y  --menu-add X  --edit new|ID
    // --label TEXT [--no-click 1]  --add LOCATION  --extract 1  --hover X,Y
    // --no-sidebar 1  --screenshot FILE
    Timer {
        interval: 900
        running: arg("--screenshot") !== undefined
        onTriggered: {
            const c = window.current
            const v = window.currentView
            if (!c) { console.log("no document open"); shot.start(); return }
            if (arg("--goto")) { const r = arg("--goto").split(","); c.show_range(Number(r[0]), Number(r[1])) }
            if (arg("--mode")) c.switch_view(arg("--mode"))
            if (arg("--no-sidebar")) window.sidebarOpen = false
            if (arg("--circular") && !c.circular) c.switch_view("circular")
            if (arg("--type")) {
                c.press(v.width / 2, 45, 0)
                c.release()
                for (const ch of arg("--type")) c.key(ch.toUpperCase().charCodeAt(0), ch, 0)
            }
            if (arg("--select")) {
                const r = arg("--select").split(",")
                const y = r.length > 2 ? Number(r[2]) : 45
                c.press(Number(r[0]), y, 0); c.drag(Number(r[1]), y); c.release()
            }
            if (arg("--add")) {
                const err = c.save_feature(-1, JSON.stringify({ kind: "misc_feature", location: arg("--add"), qualifiers: [{ key: "label", value: "partial test" }] }))
                if (err.length > 0) console.log("add failed:", err)
            }
            if (arg("--extract")) console.log("extracted:", c.extract_selection(), "|", c.message)
            if (arg("--hover")) {
                const h = arg("--hover").split(",")
                console.log("hover:", c.hover(Number(h[0]), Number(h[1])))
            }
            if (arg("--menu")) {
                const p = arg("--menu").split(",")
                menu.info = JSON.parse(c.context_press(Number(p[0]), Number(p[1])))
                menu.popup(Number(p[0]), Number(p[1]) + v.mapToItem(null, 0, 0).y)
            }
            if (arg("--menu-add")) {
                menu.info = JSON.parse(c.context_press(Number(arg("--menu-add")), 45))
                menu.popup(Number(arg("--menu-add")), 45 + v.mapToItem(null, 0, 0).y)
                menu.itemAt(5).triggered()
                menu.close()
            }
            if (arg("--edit")) {
                if (arg("--edit") === "new") editor.openNew(); else editor.openFeature(Number(arg("--edit")))
            }
            if (arg("--label")) { labelTyper.start(); return }
            shot.start()
        }
    }
    Timer {
        id: labelTyper
        interval: 300
        onTriggered: {
            const field = editor.valueField(0)
            if (!field) { console.log("no value field"); Qt.quit(); return }
            const keys = arg("--label").replace(/\\t/g, "\t")
            if (arg("--no-click") === undefined) {
                const p = field.mapToItem(null, field.width / 2, field.height / 2)
                window.currentView.clickAt(p.x, p.y)
            }
            window.currentView.typeText(keys)
            console.log("field text:", JSON.stringify(field.text), "focus:", field.activeFocus)
            shot.start()
        }
    }
    Timer {
        id: shot
        interval: 400
        onTriggered: {
            const c = window.current
            if (c) console.log(c.memory_report(), "|", c.status)
            const v = window.currentView
            console.log("saved", v ? v.saveWindow(arg("--screenshot")) : false)
            Qt.quit()
        }
    }
}
