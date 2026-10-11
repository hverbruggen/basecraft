import QtCore
import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Window
import Qt.labs.qmlmodels
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
    // "sequence" or "alignment", for the active tab.
    property string currentKind: ""
    property bool sidebarOpen: true
    property bool showGc: true
    property int ribbonTab: 0
    property string defaultLayout: "wrapped"
    property int orfMinCodons: 100
    // Typing in the alignment editor inserts bases (true) or types over
    // them (false). Shared by all alignment tabs.
    property bool alignmentInsert: true
    readonly property bool alignmentOpen: currentKind === "alignment"
    readonly property bool treeOpen: currentKind === "tree"
    readonly property var ribbonTabs: ["File", "View", alignmentOpen ? "Alignment" : treeOpen ? "Tree" : "Annotations", "Settings", "Help"]
    // Only the wrapped layout is offered for now; files always open in it.

    // Preferences, kept between sessions (~/.config/Basecraft).
    Settings {
        category: "ui"
        property alias showGc: window.showGc
        property alias sidebarOpen: window.sidebarOpen
        property alias orfMinCodons: window.orfMinCodons
        property alias alignmentInsert: window.alignmentInsert
    }

    function fileName(path) { return path.split("/").pop() }

    // This run's activity in the open files' projects, as a text file in
    // the default text editor.
    function showActivity() {
        const files = []
        for (let i = 0; i < openFiles.count; i++) files.push(openFiles.get(i).path)
        const path = shell.activity_log(files.join("\n"), -new Date().getTimezoneOffset())
        if (path.length > 0) Qt.openUrlExternally("file://" + path)
    }
    function localPath(url) { return decodeURIComponent(url.toString().replace(/^file:\/\//, "")) }

    function refreshCurrent() {
        const d = docs.itemAt(tabs.currentIndex)
        current = d ? d.controller : null
        currentView = d ? d.view : null
        currentPath = d ? d.path : ""
        currentKind = d ? d.kind : ""
        if (currentView) currentView.forceActiveFocus()
    }

    // Open a file in a tab, or switch to its tab if it's already open.
    function openPath(path) {
        for (let i = 0; i < openFiles.count; i++) {
            if (openFiles.get(i).path === path) { tabs.currentIndex = i; return }
        }
        if (!shell.project_dir.length) shell.use_project_of(path)
        openFiles.append({ path: path, dirty: false, kind: shell.file_kind(path) })
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

    // Ribbon pieces: a labelled group of controls, and its buttons.
    // `tip` says in a few words what the button does; `keys` is its shortcut.
    component RibbonButton: ToolButton {
        property string tip
        property string keys
        ToolTip.text: keys.length > 0 ? tip + "   " + keys : tip
        ToolTip.visible: hovered && tip.length > 0
        ToolTip.delay: 400
        focusPolicy: Qt.NoFocus
        enabled: window.current !== null
        display: AbstractButton.TextOnly
        topPadding: 4
        bottomPadding: 4
    }
    component RibbonGroup: ColumnLayout {
        id: group
        property string title
        default property alias content: row.data
        spacing: 0
        Layout.fillHeight: true
        RowLayout {
            id: row
            spacing: 2
            Layout.fillHeight: true
            Layout.alignment: Qt.AlignVCenter
        }
        Label {
            text: group.title
            Layout.alignment: Qt.AlignHCenter
            font.pixelSize: 9
            opacity: 0.55
        }
    }
    // The same last group on every ribbon tab.
    // A ribbon button that shows whether its mode is on.
    component ModeButton: RibbonButton {
        id: modeButton
        property bool active
        background: Rectangle {
            radius: 3
            color: modeButton.active ? Qt.alpha(shell.accent, 0.18)
                : modeButton.hovered ? Qt.alpha(shell.foreground, 0.05) : "transparent"
            border.color: modeButton.active ? shell.accent : "transparent"
        }
    }
    component ShortcutsGroup: RibbonGroup {
        title: "Keys"
        RibbonButton {
            text: "Shortcuts…"
            tip: "Keyboard shortcuts for this tab"
            enabled: true
            onClicked: shortcuts.show(window.ribbonTabs[window.ribbonTab])
        }
    }
    component RibbonTabButton: ToolButton {
        id: tabButton
        required property int tabIndex
        focusPolicy: Qt.NoFocus
        checkable: true
        checked: window.ribbonTab === tabIndex
        onClicked: window.ribbonTab = tabIndex
        topPadding: 3
        bottomPadding: 3
        leftPadding: 12
        rightPadding: 12
        background: Rectangle {
            color: tabButton.checked ? Qt.alpha(shell.foreground, 0.07) : (tabButton.hovered ? Qt.alpha(shell.foreground, 0.04) : "transparent")
            Rectangle {
                visible: tabButton.checked
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                height: 2
                color: shell.accent
            }
        }
    }

    header: ColumnLayout {
        spacing: 0

        // Ribbon tabs. Undo, redo and save are Ctrl+Z, Ctrl+Shift+Z, Ctrl+S.
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 26
            color: Qt.alpha(shell.foreground, 0.05)
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 6
                spacing: 0
                RibbonTabButton { tabIndex: 0; text: "File" }
                RibbonTabButton { tabIndex: 1; text: "View" }
                RibbonTabButton { tabIndex: 2; text: window.ribbonTabs[2] }
                RibbonTabButton { tabIndex: 3; text: "Settings" }
                RibbonTabButton { tabIndex: 4; text: "Help" }
                Label {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignRight
                    rightPadding: 10
                    elide: Text.ElideLeft
                    text: window.current ? window.current.message : ""
                    // Green for done, red for failed, plain for hints.
                    color: !window.current ? shell.foreground
                        : window.current.message_kind === "ok" ? window.current.ok_color
                        : window.current.message_kind === "error" ? window.current.error_color
                        : shell.foreground
                    // A message goes away by itself after 10 seconds.
                    Timer {
                        id: messageTimer
                        interval: 10000
                        onTriggered: if (window.current) window.current.message = ""
                    }
                    onTextChanged: if (text.length > 0) messageTimer.restart()
                }
                ToolButton {
                    text: "⋯"
                    focusPolicy: Qt.NoFocus
                    ToolTip.text: "More"
                    ToolTip.visible: hovered
                    onClicked: moreMenu.popup(this, 0, height)
                    Menu {
                        id: moreMenu
                        MenuItem { text: "Activity log"; onTriggered: window.showActivity() }
                        MenuItem { text: "Keyboard shortcuts"; onTriggered: shortcuts.show("") }
                        MenuSeparator {}
                        MenuItem { text: "About Basecraft"; onTriggered: about.open() }
                    }
                }
            }
        }

        // The ribbon panel for the chosen tab.
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 54
            color: Qt.alpha(shell.foreground, 0.025)
            StackLayout {
                anchors { fill: parent; leftMargin: 8; topMargin: 4; bottomMargin: 2 }
                // The third tab is Annotations for a sequence, Alignment
                // for an alignment, Tree for a tree; those two panels sit
                // last.
                currentIndex: window.ribbonTab !== 2 ? window.ribbonTab
                    : window.alignmentOpen ? 5 : window.treeOpen ? 6 : 2

                RowLayout {
                    spacing: 6
                    RibbonGroup {
                        title: "File"
                        RibbonButton { text: "Open…"; tip: "Open a sequence or alignment file"; enabled: true; onClicked: openDialog.open() }
                        RibbonButton { text: "Save"; tip: "Save the current file"; keys: "Ctrl+S"; onClicked: window.current.save() }
                        RibbonButton { text: "Save as…"; tip: "Save as a new file and carry on in that one"; keys: "Ctrl+Shift+S"; onClicked: window.saveAs() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Project"
                        RibbonButton { text: "Open project…"; tip: "Choose or create a project folder"; enabled: true; onClicked: projectDialog.open() }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                RowLayout {
                    spacing: 6
                    RibbonGroup {
                        title: "Zoom"
                        RibbonButton { text: "−"; tip: "Zoom out"; keys: "Ctrl+−"; onClicked: window.current.zoom_out() }
                        RibbonButton { text: "+"; tip: "Zoom in"; keys: "Ctrl++"; onClicked: window.current.zoom_in() }
                        RibbonButton {
                            text: "Fit"
                            tip: window.alignmentOpen ? "Show the whole width of the alignment" : window.treeOpen ? "Show the whole tree" : "Show the whole sequence"
                            keys: "Ctrl+0"
                            onClicked: window.current.fit()
                        }
                        RibbonButton { text: "Selection"; visible: !window.treeOpen; tip: "Zoom to the selection"; onClicked: window.current.zoom_to_selection() }
                        Label {
                            text: window.current ? window.current.zoom_text : ""
                            leftPadding: 6
                            opacity: 0.7
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    // Linear and circular layouts exist in the code but are
                    // not offered for now; the wrapped layout is the view.
                    RibbonGroup {
                        title: "Show"
                        CheckBox {
                            text: "GC content"
                            enabled: window.currentKind === "sequence"
                            ToolTip.text: "Show the GC content plot"
                            ToolTip.visible: hovered
                            ToolTip.delay: 400
                            checked: window.showGc
                            focusPolicy: Qt.NoFocus
                            onToggled: window.showGc = checked
                        }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                RowLayout {
                    spacing: 6
                    RibbonGroup {
                        title: "Annotation"
                        RibbonButton { text: "Add…"; tip: "Annotate the selection"; keys: "Ctrl+N"; onClicked: editor.openNew() }
                        RibbonButton { text: "Edit…"; tip: "Edit the selected annotation"; keys: "Enter"; onClicked: editor.openSelected() }
                        RibbonButton {
                            text: "Delete"
                            tip: "Delete the selected annotation"
                            keys: "Delete"
                            onClicked: {
                                const id = window.current.selected_feature()
                                if (id >= 0) window.current.delete_feature(id)
                                else { window.current.message_kind = ""; window.current.message = "Click an annotation first" }
                            }
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Go to"
                        RibbonButton { text: "◀ Previous"; tip: "Select the previous annotation"; onClicked: window.current.step_feature(false) }
                        RibbonButton { text: "Next ▶"; tip: "Select the next annotation"; onClicked: window.current.step_feature(true) }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Sequence"
                        RibbonButton {
                            text: "Copy"
                            tip: "Copy the selected annotation's sequence"
                            keys: "Ctrl+C"
                            onClicked: {
                                const id = window.current.selected_feature()
                                if (id >= 0) clipboard.copy(window.current.feature_sequence(id))
                                else { window.current.message_kind = ""; window.current.message = "Click an annotation first" }
                            }
                        }
                        RibbonButton { text: "Extract"; tip: "Save the selection as a new file"; keys: "Ctrl+E"; onClicked: window.current.extract_selection() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Find"
                        RibbonButton {
                            text: "Find ORFs…"
                            tip: "Annotate open reading frames"
                            onClicked: orfDialog.search(false)
                        }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                RowLayout {
                    spacing: 6
                    RibbonGroup {
                        title: "Window"
                        CheckBox {
                            text: "Files sidebar open"
                            ToolTip.text: "Show the files sidebar   Ctrl+B"
                            ToolTip.visible: hovered
                            ToolTip.delay: 400
                            checked: window.sidebarOpen
                            focusPolicy: Qt.NoFocus
                            onToggled: window.sidebarOpen = checked
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Appearance"
                        Label { text: "Colours and font follow the Omarchy theme"; opacity: 0.7 }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                RowLayout {
                    spacing: 6
                    RibbonGroup {
                        title: "Learn"
                        RibbonButton { text: "Keyboard shortcuts…"; tip: "List all keyboard shortcuts"; enabled: true; onClicked: shortcuts.show("") }
                        RibbonButton { text: "About Basecraft…"; tip: "Version and licence"; enabled: true; onClicked: about.open() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Online"
                        RibbonButton { text: "Project page"; tip: "Open the project on GitHub"; enabled: true; onClicked: Qt.openUrlExternally("https://github.com/hverbruggen/basecraft") }
                        RibbonButton { text: "Report a problem"; tip: "Open a new GitHub issue"; enabled: true; onClicked: Qt.openUrlExternally("https://github.com/hverbruggen/basecraft/issues/new") }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                // Alignment: edits on the selected block, its columns and rows.
                RowLayout {
                    spacing: 6
                    RibbonGroup {
                        title: "Typing"
                        ModeButton {
                            text: "Insert"
                            active: window.alignmentInsert
                            tip: "Typing inserts bases; the rest of the row moves right"
                            keys: "Insert"
                            onClicked: window.alignmentInsert = true
                        }
                        ModeButton {
                            text: "Overwrite"
                            active: !window.alignmentInsert
                            tip: "Typing replaces the selected bases"
                            keys: "Insert"
                            onClicked: window.alignmentInsert = false
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Gaps"
                        RibbonButton { text: "Insert"; tip: "Insert a gap before the selection"; keys: "Space"; onClicked: window.current.insert_gap() }
                        RibbonButton { text: "Remove"; tip: "Remove the gaps just left of the selection"; keys: "Backspace"; onClicked: window.current.remove_gap() }
                        RibbonButton { text: "◀ Slide"; tip: "Slide the selection left into gaps, or drag it"; keys: "Alt+←"; onClicked: window.current.slide(-1) }
                        RibbonButton { text: "Slide ▶"; tip: "Slide the selection right into gaps, or drag it"; keys: "Alt+→"; onClicked: window.current.slide(1) }
                        RibbonButton { text: "Delete"; tip: "Delete the selected bases; the rows close up"; keys: "Delete"; onClicked: window.current.delete_selection() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Columns"
                        RibbonButton { text: "Insert"; tip: "Insert a gap column before the selection"; onClicked: window.current.insert_column() }
                        RibbonButton { text: "Delete"; tip: "Delete the selected columns from every row"; onClicked: window.current.delete_columns() }
                        RibbonButton { text: "Strip gaps"; tip: "Delete every column that is a gap in all rows"; onClicked: window.current.remove_gap_columns() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Rows"
                        RibbonButton { text: "▲"; tip: "Move the selected rows up"; keys: "Alt+↑"; onClicked: window.current.move_rows(true) }
                        RibbonButton { text: "▼"; tip: "Move the selected rows down"; keys: "Alt+↓"; onClicked: window.current.move_rows(false) }
                        RibbonButton { text: "To top"; tip: "Move the selected rows to the top"; onClicked: window.current.rows_to_top() }
                        RibbonButton { text: "Rename…"; tip: "Rename the cursor's row"; keys: "F2"; onClicked: renameDialog.ask() }
                        RibbonButton { text: "Delete"; tip: "Delete the selected rows"; onClicked: window.current.remove_rows() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Align"
                        RibbonButton {
                            text: window.current && window.current.busy ? "Aligning…" : "MAFFT"
                            enabled: window.current !== null && !window.current.busy
                            tip: "Realign the sequences with MAFFT, default settings, into a new file"
                            onClicked: window.current.align_mafft()
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Selection"
                        RibbonButton { text: "All"; tip: "Select everything"; keys: "Ctrl+A"; onClicked: window.current.select_all() }
                        RibbonButton {
                            text: "Copy"
                            tip: "Copy the selected block as FASTA"
                            keys: "Ctrl+C"
                            onClicked: clipboard.copy(window.current.selection_text())
                        }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                // Tree: the selected clade, the order of clades, support
                // values and figures.
                RowLayout {
                    spacing: 6
                    RibbonGroup {
                        title: "Clade"
                        RibbonButton { text: "Annotate…"; tip: "Name the selected clade, with a bar or a coloured field"; keys: "Enter"; enabled: window.current !== null && window.current.has_selection; onClicked: cladeEditor.openSelected() }
                        RibbonButton { text: "Reroot"; tip: "Root the tree on the branch above the selection"; enabled: window.current !== null && window.current.has_selection; onClicked: window.current.reroot() }
                        RibbonButton { text: "Rotate"; tip: "Swap the order of the selected clade's branches"; enabled: window.current !== null && window.current.has_selection; onClicked: window.current.rotate() }
                        RibbonButton { text: "Rename…"; tip: "Rename the selected tip"; keys: "F2"; enabled: window.current !== null && window.current.tip_selected; onClicked: tipDialog.ask() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Order"
                        RibbonButton { text: "Small on top"; tip: "Order every clade's branches by size, smallest first"; onClicked: window.current.ladderize(true) }
                        RibbonButton { text: "Large on top"; tip: "Order every clade's branches by size, largest first"; onClicked: window.current.ladderize(false) }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Support"
                        RibbonButton { text: "Add from tree…"; tip: "Add support values from another analysis of the same taxa, matched branch by branch"; onClicked: supportFile.open() }
                        ModeButton { text: "Natural"; tip: "Above branches going up, below branches going down"; active: window.current !== null && window.current.support_placement === "natural"; onClicked: window.current.place_support("natural") }
                        ModeButton { text: "Above"; tip: "All support values above their branch"; active: window.current !== null && window.current.support_placement === "above"; onClicked: window.current.place_support("above") }
                        ModeButton { text: "Below"; tip: "All support values below their branch"; active: window.current !== null && window.current.support_placement === "below"; onClicked: window.current.place_support("below") }
                        RibbonButton { text: "Style…"; tip: "Which support values show, circles at nodes, sizes"; onClicked: treeStyle.openStyle() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Figure"
                        RibbonButton { text: "Export SVG…"; tip: "Save the tree as an SVG figure, black on white"; onClicked: window.exportSvg() }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }
            }
        }
    }

    ShortcutsDialog { id: shortcuts }

    Dialog {
        id: orfDialog
        property bool inSelection: false
        function search(selectionOnly) { inSelection = selectionOnly; open() }
        title: inSelection ? "Find open reading frames in the selection" : "Find open reading frames"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: codons.forceActiveFocus()
        onAccepted: {
            window.orfMinCodons = codons.value
            window.current.find_orfs(codons.value, orfDialog.inSelection)
        }
        ColumnLayout {
            spacing: 8
            RowLayout {
                Label { text: "At least" }
                SpinBox {
                    id: codons
                    from: 10
                    to: 10000
                    stepSize: 10
                    editable: true
                    value: window.orfMinCodons
                    Keys.onReturnPressed: orfDialog.accept()
                    Keys.onEnterPressed: orfDialog.accept()
                }
                Label { text: "codons, start to stop" }
            }
            Label {
                text: "Searches both strands. Each ORF is added as a misc_feature with the note \"ORF\";\none Undo removes them all."
                opacity: 0.7
            }
        }
    }

    Dialog {
        id: about
        title: "About Basecraft"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Close
        ColumnLayout {
            spacing: 6
            Label { text: "Basecraft " + shell.version(); font.bold: true }
            Label { text: "A workbench for sequences and their annotations." }
            Label { text: "Every edit is logged in .basecraft/ next to your files, and can be replayed."; opacity: 0.7 }
            Label { text: "GPL-3.0-or-later; plugin protocol and Python SDK Apache-2.0."; opacity: 0.7 }
        }
    }

    FileDialog {
        id: openDialog
        title: "Open a sequence, alignment or tree"
        currentFolder: shell.project_dir.length ? "file://" + shell.project_dir : ""
        nameFilters: ["Sequences, alignments and trees (*.gb *.gbk *.genbank *.gbff *.fasta *.fa *.fna *.fas *.ffn *.faa *.afa *.nwk *.newick *.tre *.tree *.trees *.treefile *.contree *.nex *.nexus *.nxs)", "All files (*)"]
        onAccepted: window.openPath(window.localPath(selectedFile))
    }

    // Save the active tab under a new name; the tab then shows the new file.
    function saveAs() {
        if (!window.current) return
        saveDialog.index = tabs.currentIndex
        saveDialog.selectedFile = "file://" + window.currentPath
        saveDialog.open()
    }
    Shortcut { sequence: "Ctrl+Shift+S"; onActivated: window.saveAs() }
    FileDialog {
        id: saveDialog
        property int index: -1
        title: "Save as"
        fileMode: FileDialog.SaveFile
        currentFolder: shell.project_dir.length ? "file://" + shell.project_dir : ""
        onAccepted: {
            const d = docs.itemAt(index)
            const path = window.localPath(selectedFile)
            if (d && d.controller.save_as(path)) {
                openFiles.setProperty(index, "path", path)
                Qt.callLater(window.refreshCurrent)
            }
        }
    }

    FolderDialog {
        id: projectDialog
        title: "Open or create a project folder"
        currentFolder: shell.project_dir.length ? "file://" + shell.project_dir : ""
        onAccepted: shell.project_dir = window.localPath(selectedFolder)
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
                                // the extension; the path in the project is a
                                // hover away.
                                text: window.fileName(path) + (dirty ? " •" : "")
                                font.pixelSize: 10
                                width: Math.min(implicitWidth, 180)
                                elide: Text.ElideMiddle
                                anchors.verticalCenter: parent.verticalCenter
                                ToolTip.text: shell.relative_path(path)
                                ToolTip.visible: tabHover.hovered
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
                    // Each file opens in the editor for what it holds.
                    delegate: DelegateChooser {
                        role: "kind"
                        DelegateChoice {
                            roleValue: "alignment"
                            AlignmentView {
                                insertMode: window.alignmentInsert
                                onToggleInsert: window.alignmentInsert = !window.alignmentInsert
                                onOpenFile: (path) => window.openPath(path)
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                                onContextMenu: (info) => { alnMenu.info = info; alnMenu.popup() }
                                onCopyText: (text) => clipboard.copy(text)
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                                onRenameRow: renameDialog.ask()
                            }
                        }
                        DelegateChoice {
                            roleValue: "tree"
                            // Not "TreeView": that's Qt's, used by the
                            // files sidebar.
                            PhyloView {
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                                onContextMenu: (info) => { treeMenu.info = info; treeMenu.popup() }
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                                onRenameTip: tipDialog.ask()
                                onAnnotate: cladeEditor.openSelected()
                            }
                        }
                        DelegateChoice {
                            DocumentView {
                                showGc: window.showGc
                                defaultLayout: window.defaultLayout
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                                onContextMenu: (info) => { menu.info = info; menu.popup() }
                                onEditFeature: (id) => editor.openFeature(id)
                                onEditSelected: editor.openSelected()
                                onNewAnnotation: editor.openNew()
                                onCopyText: (text) => clipboard.copy(text)
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                            }
                        }
                    }
                }
            }

            Label {
                visible: openFiles.count === 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                text: "Open a sequence or an alignment from the files on the left"
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
        Item { text: "Find ORFs in selection…"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: orfDialog.search(true) }
    }

    // Alignment context menu: what applies to the cells, rows or columns
    // under the pointer.
    Menu {
        id: alnMenu
        property var info: ({ region: "grid", rows: 1, columns: 1, rowName: "" })
        readonly property bool cells: info.region === "grid"
        readonly property bool rows: info.region === "names" || info.region === "corner"
        readonly property bool columns: info.region === "header"
        component AlnItem: MenuItem { height: visible ? implicitHeight : 0 }

        AlnItem { text: "Insert gap"; visible: alnMenu.cells; onTriggered: window.current.insert_gap() }
        AlnItem { text: "Remove gap to the left"; visible: alnMenu.cells; onTriggered: window.current.remove_gap() }
        AlnItem { text: "Delete " + alnMenu.info.rows + " × " + alnMenu.info.columns + " bases"; visible: alnMenu.cells; onTriggered: window.current.delete_selection() }
        AlnItem { text: "Rename “" + alnMenu.info.rowName + "”…"; visible: alnMenu.info.rows === 1 && !alnMenu.columns; onTriggered: renameDialog.ask() }
        AlnItem { text: "Move to top"; visible: alnMenu.rows; onTriggered: window.current.rows_to_top() }
        AlnItem { text: alnMenu.info.rows === 1 ? "Delete row" : "Delete " + alnMenu.info.rows + " rows"; visible: alnMenu.rows; onTriggered: window.current.remove_rows() }
        AlnItem { text: "Insert gap column"; visible: alnMenu.columns; onTriggered: window.current.insert_column() }
        AlnItem { text: alnMenu.info.columns === 1 ? "Delete column" : "Delete " + alnMenu.info.columns + " columns"; visible: alnMenu.columns; onTriggered: window.current.delete_columns() }
        AlnItem { text: "Strip gap-only columns"; visible: alnMenu.columns || alnMenu.rows; onTriggered: window.current.remove_gap_columns() }
        AlnItem { text: "Copy as FASTA"; onTriggered: clipboard.copy(window.current.selection_text()) }
    }

    Dialog {
        id: renameDialog
        title: "Rename row"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        function ask() {
            if (!window.alignmentOpen) return
            rowName.text = window.current.row_name()
            renameError.text = ""
            open()
        }
        onOpened: { rowName.forceActiveFocus(); rowName.selectAll() }
        onAccepted: {
            const err = window.current.rename_row(rowName.text)
            if (err.length > 0) { renameError.text = err; open() }
        }
        onClosed: if (window.currentView) window.currentView.forceActiveFocus()
        ColumnLayout {
            spacing: 6
            TextField {
                id: rowName
                Layout.preferredWidth: 320
                Keys.onReturnPressed: renameDialog.accept()
                Keys.onEnterPressed: renameDialog.accept()
            }
            Label { id: renameError; visible: text.length > 0; color: shell.accent }
        }
    }

    // Tree context menu: what applies to the clade or tip under the pointer.
    Menu {
        id: treeMenu
        property var info: ({ node: false, tip: false, root: false, annotated: false, annotation: 0 })
        component TreeItem: MenuItem { height: visible ? implicitHeight : 0 }

        TreeItem { text: treeMenu.info.annotated ? "Edit annotation…" : "Annotate clade…"; visible: treeMenu.info.node; onTriggered: cladeEditor.openSelected() }
        TreeItem { text: "Remove annotation"; visible: treeMenu.info.annotated; onTriggered: window.current.remove_annotation(treeMenu.info.annotation) }
        TreeItem { text: "Reroot here"; visible: treeMenu.info.node && !treeMenu.info.root; onTriggered: window.current.reroot() }
        TreeItem { text: "Rotate"; visible: treeMenu.info.node && !treeMenu.info.tip; onTriggered: window.current.rotate() }
        TreeItem { text: "Rename tip…"; visible: treeMenu.info.tip; onTriggered: tipDialog.ask() }
        TreeItem { text: "Add support from tree…"; visible: !treeMenu.info.node; onTriggered: supportFile.open() }
        TreeItem { text: "Tree style…"; visible: !treeMenu.info.node; onTriggered: treeStyle.openStyle() }
    }

    Dialog {
        id: tipDialog
        title: "Rename tip"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        function ask() {
            if (!window.treeOpen || !window.current.tip_selected) return
            tipName.text = window.current.tip_name()
            tipError.text = ""
            open()
        }
        onOpened: { tipName.forceActiveFocus(); tipName.selectAll() }
        onAccepted: {
            const err = window.current.rename_tip(tipName.text)
            if (err.length > 0) { tipError.text = err; open() }
        }
        onClosed: if (window.currentView) window.currentView.forceActiveFocus()
        ColumnLayout {
            spacing: 6
            TextField {
                id: tipName
                Layout.preferredWidth: 320
                Keys.onReturnPressed: tipDialog.accept()
                Keys.onEnterPressed: tipDialog.accept()
            }
            Label { id: tipError; visible: text.length > 0; color: shell.accent }
        }
    }

    // Support from another tree: pick the file, then name the values.
    FileDialog {
        id: supportFile
        title: "Add support values from another tree"
        currentFolder: shell.project_dir.length ? "file://" + shell.project_dir : ""
        nameFilters: ["Trees (*.nwk *.newick *.tre *.tree *.trees *.treefile *.contree *.nex *.nexus *.nxs)", "All files (*)"]
        onAccepted: { supportName.path = window.localPath(selectedFile); supportName.open() }
    }
    Dialog {
        id: supportName
        property string path
        title: "Name the support values"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: { supportNameField.text = ""; supportNameField.forceActiveFocus() }
        onAccepted: window.current.add_support_from(path, supportNameField.text)
        onClosed: if (window.currentView) window.currentView.forceActiveFocus()
        ColumnLayout {
            spacing: 6
            Label { text: window.fileName(supportName.path) }
            TextField {
                id: supportNameField
                Layout.preferredWidth: 320
                placeholderText: "e.g. Bootstrap, PP, UFBoot; empty keeps the file's names"
                Keys.onReturnPressed: supportName.accept()
                Keys.onEnterPressed: supportName.accept()
            }
        }
    }

    CladeEditor {
        id: cladeEditor
        controller: window.treeOpen ? window.current : null
        onClosed: if (window.currentView) window.currentView.forceActiveFocus()
    }
    TreeStyleDialog {
        id: treeStyle
        controller: window.treeOpen ? window.current : null
        onClosed: if (window.currentView) window.currentView.forceActiveFocus()
    }

    function exportSvg() {
        if (!window.treeOpen) return
        svgDialog.selectedFile = "file://" + window.currentPath.replace(/\.[^./]*$/, "") + ".svg"
        svgDialog.open()
    }
    FileDialog {
        id: svgDialog
        title: "Export the tree as SVG"
        fileMode: FileDialog.SaveFile
        nameFilters: ["SVG figures (*.svg)"]
        onAccepted: window.current.export_svg(window.localPath(selectedFile))
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
        // Annotations belong to sequences; alignments have none yet.
        controller: window.currentKind === "sequence" ? window.current : null
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
    // --no-sidebar 1  --fit 1  --aln STEPS  --shot-delay MS  --screenshot FILE
    Timer {
        interval: 900
        running: arg("--screenshot") !== undefined
        onTriggered: {
            const c = window.current
            const v = window.currentView
            if (!c) { console.log("no document open"); shot.start(); return }
            if (arg("--fit")) c.fit()
            // Alignment edits, ';'-separated: sel:R0,C0,R1,C1  gap  rmgap
            // slide:N  type:TEXT  del  undo  redo  up  down  strip  zoomsel
            // insert  overwrite  mafft
            if (arg("--aln")) {
                for (const step of arg("--aln").split(";")) {
                    const [cmd, a] = step.split(":")
                    if (cmd === "sel") { const n = a.split(",").map(Number); c.select_block(n[0], n[1], n[2], n[3]) }
                    else if (cmd === "gap") c.insert_gap()
                    else if (cmd === "rmgap") c.remove_gap()
                    else if (cmd === "slide") c.slide(Number(a))
                    else if (cmd === "type") { for (const ch of a) c.key(ch.toUpperCase().charCodeAt(0), ch, 0) }
                    else if (cmd === "del") c.delete_selection()
                    else if (cmd === "undo") c.undo()
                    else if (cmd === "redo") c.redo()
                    else if (cmd === "up") c.move_rows(true)
                    else if (cmd === "down") c.move_rows(false)
                    else if (cmd === "strip") c.remove_gap_columns()
                    else if (cmd === "zoomsel") c.zoom_to_selection()
                    else if (cmd === "insert") window.alignmentInsert = true
                    else if (cmd === "overwrite") window.alignmentInsert = false
                    else if (cmd === "mafft") c.align_mafft()
                    console.log("aln", step, "|", c.message)
                }
            }
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
            if (arg("--ribbon")) window.ribbonTab = Number(arg("--ribbon"))
            if (arg("--orfs")) console.log(window.current.find_orfs(Number(arg("--orfs")), arg("--select") !== undefined))
            if (arg("--activity")) {
                const files = []
                for (let i = 0; i < openFiles.count; i++) files.push(openFiles.get(i).path)
                console.log("activity:", shell.activity_log(files.join("\n"), -new Date().getTimezoneOffset()))
            }
            if (arg("--shortcuts")) shortcuts.show(window.ribbonTabs[window.ribbonTab])
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
        interval: arg("--shot-delay") ? Number(arg("--shot-delay")) : 400
        onTriggered: {
            const c = window.current
            if (c) console.log(c.memory_report(), "|", c.status, "|", c.hscroll_size, c.vscroll_size)
            const v = window.currentView
            console.log("saved", v ? v.saveWindow(arg("--screenshot")) : false)
            Qt.quit()
        }
    }
}
