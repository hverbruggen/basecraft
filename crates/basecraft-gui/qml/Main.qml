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

    // Closing the window (or Ctrl+Q) asks about unsaved changes first.
    property bool closeConfirmed: false
    onClosing: (close) => {
        if (closeConfirmed) return
        const unsaved = window.unsavedTabs()
        if (unsaved.length === 0) return
        close.accepted = false
        quitDialog.ask(unsaved)
    }
    function unsavedTabs() {
        const out = []
        for (let i = 0; i < openFiles.count; i++) {
            const d = docs.itemAt(i)
            if (d && d.controller.dirty) out.push(i)
        }
        return out
    }

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
    // The chosen ribbon tab, by name.
    property string ribbonTabName: "File"
    property string defaultLayout: "wrapped"
    property int orfMinCodons: 100
    // Typing in the alignment editor inserts bases (true) or types over
    // them (false). Shared by all alignment tabs.
    property bool alignmentInsert: true
    readonly property bool alignmentOpen: currentKind === "alignment"
    readonly property bool treeOpen: currentKind === "tree"
    readonly property bool delimitationOpen: currentKind === "delimitation"
    // The third tab is Annotations for a sequence, Alignment for an
    // alignment, Tree for a tree; an alignment also gets Build phylogeny.
    readonly property var ribbonTabs: ["File", "View", alignmentOpen ? "Alignment" : treeOpen ? "Tree" : delimitationOpen ? "Delimitation" : "Annotations"]
        .concat(alignmentOpen ? ["Build phylogeny"] : [], ["Settings", "Help"])
    // The chosen tab if this document has it, else its third tab (a tab
    // for another kind of document), else File.
    readonly property string ribbonTab: ribbonTabs.includes(ribbonTabName) ? ribbonTabName
        : ["Annotations", "Alignment", "Tree", "Build phylogeny", "Delimitation"].includes(ribbonTabName) ? ribbonTabs[2] : "File"
    // Each tab's panel in the ribbon's stack.
    readonly property var ribbonPanels: ({ "File": 0, "View": 1, "Annotations": 2, "Settings": 3, "Help": 4, "Alignment": 5, "Tree": 6, "Build phylogeny": 7, "Delimitation": 8 })
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

    Connections {
        target: window.current
        ignoreUnknownSignals: true
        function onTaxaSelected(names) {
            if (!window.current) return
            const source = (window.treeOpen || window.delimitationOpen) ? window.current.linked_alignment : window.current.file_path
            if (!source) return
            for (let i = 0; i < docs.count; i++) {
                const d = docs.itemAt(i)
                if (!d || d.controller === window.current) continue
                if ((window.treeOpen || window.delimitationOpen) && d.kind === "alignment" && d.controller.file_path === source)
                    d.controller.highlight_taxa(names)
                else if (window.alignmentOpen && (d.kind === "tree" || d.kind === "delimitation") && d.controller.linked_alignment === source)
                    d.controller.highlight_taxa(names)
            }
        }
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
        id: ribbonButton
        property string tip
        property string keys
        // A Nerd Font glyph shown before the text, or alone.
        property string glyph
        ToolTip.text: keys.length > 0 ? tip + "   " + keys : tip
        ToolTip.visible: hovered && tip.length > 0
        ToolTip.delay: 400
        focusPolicy: Qt.NoFocus
        enabled: window.current !== null
        display: AbstractButton.TextOnly
        topPadding: 4
        bottomPadding: 4
        contentItem: Row {
            spacing: 5
            opacity: ribbonButton.enabled ? 1 : 0.4
            Label {
                visible: ribbonButton.glyph.length > 0
                anchors.verticalCenter: parent.verticalCenter
                text: ribbonButton.glyph
                font.family: "JetBrainsMono Nerd Font"
                font.pixelSize: ribbonButton.font.pixelSize + 3
                color: ribbonButton.palette.buttonText
            }
            Label {
                visible: ribbonButton.text.length > 0
                anchors.verticalCenter: parent.verticalCenter
                text: ribbonButton.text
                font: ribbonButton.font
                color: ribbonButton.palette.buttonText
            }
        }
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
            glyph: "\uea65"
            tip: "Keyboard shortcuts for this tab"
            enabled: true
            onClicked: shortcuts.show(window.ribbonTab)
        }
    }
    component RibbonTabButton: ToolButton {
        id: tabButton
        required property string modelData
        focusPolicy: Qt.NoFocus
        checkable: true
        text: modelData
        checked: window.ribbonTab === modelData
        onClicked: window.ribbonTabName = modelData
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
                Repeater {
                    model: window.ribbonTabs
                    RibbonTabButton {}
                }
                Label {
                    id: messageLabel
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
                    onTextChanged: if (messageLabel.text.length > 0) messageTimer.restart()
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
            implicitHeight: ribbonScroll.contentWidth > ribbonScroll.width ? 64 : 54
            color: Qt.alpha(shell.foreground, 0.025)
            Flickable {
                id: ribbonScroll
                anchors { fill: parent; leftMargin: 8; topMargin: 4; bottomMargin: 2 }
                clip: true
                contentWidth: Math.max(width, ribbonStack.children[ribbonStack.currentIndex].implicitWidth)
                contentHeight: 48
                flickableDirection: Flickable.HorizontalFlick
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.horizontal: ScrollBar { policy: ScrollBar.AsNeeded; active: true }
            StackLayout {
                id: ribbonStack
                width: ribbonScroll.contentWidth
                height: ribbonScroll.contentHeight
                currentIndex: window.ribbonPanels[window.ribbonTab]
                onCurrentIndexChanged: ribbonScroll.contentX = 0

                RowLayout {
                    spacing: 6
                    RibbonGroup {
                        title: "File"
                        RibbonButton { text: "Open…"; glyph: "\ueaf7"; tip: "Open a sequence or alignment file"; enabled: true; onClicked: openDialog.open() }
                        RibbonButton { text: "Save"; glyph: "\ueb4b"; tip: "Save the current file"; keys: "Ctrl+S"; onClicked: window.current.save() }
                        RibbonButton { text: "Save as…"; glyph: "\ueb4a"; tip: "Save as a new file and carry on in that one"; keys: "Ctrl+Shift+S"; onClicked: window.saveAs() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Project"
                        RibbonButton { text: "Open project…"; glyph: "\uebdf"; tip: "Choose or create a project folder"; enabled: true; onClicked: projectDialog.open() }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                RowLayout {
                    spacing: 6
                    RibbonGroup {
                        title: "Zoom"
                        RibbonButton { glyph: "\ueb82"; tip: "Zoom out"; keys: "Ctrl+−"; onClicked: (window.delimitationOpen ? window.currentView.controller : window.current).zoom_out() }
                        RibbonButton { glyph: "\ueb81"; tip: "Zoom in"; keys: "Ctrl++"; onClicked: (window.delimitationOpen ? window.currentView.controller : window.current).zoom_in() }
                        RibbonButton {
                            text: "Fit"
                            glyph: "\ueb4c"
                            tip: window.alignmentOpen ? "Show the whole width of the alignment" : window.treeOpen ? "Show the whole tree" : "Show the whole sequence"
                            keys: "Ctrl+0"
                            onClicked: (window.delimitationOpen ? window.currentView.controller : window.current).fit()
                        }
                        RibbonButton { text: "Selection"; glyph: "\uebf8"; visible: !window.treeOpen && !window.delimitationOpen; tip: "Zoom to the selection"; onClicked: window.current.zoom_to_selection() }
                        Label {
                            text: window.current ? window.current.zoom_text : ""
                            leftPadding: 6
                            opacity: 0.7
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true; visible: window.currentKind === "sequence" }
                    // Linear and circular layouts exist in the code but are
                    // not offered for now; the wrapped layout is the view.
                    RibbonGroup {
                        title: "Show"
                        // Only sequences have a GC plot.
                        visible: window.currentKind === "sequence"
                        CheckBox {
                            text: "GC content"
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
                        RibbonButton {
                            text: "Predict genes…"
                            tip: "Annotate protein-coding genes with Prodigal"
                            onClicked: genesDialog.predict(false)
                        }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                RowLayout {
                    spacing: 6
                    RibbonGroup {
                        title: "Window"
                        RibbonButton {
                            text: "Move to window"
                            enabled: window.current !== null && !window.current.dirty && !window.current.busy
                            tip: "Move this saved document to its own window; save edits first"
                            onClicked: if (shell.open_window(window.currentPath)) window.closeTab(tabs.currentIndex)
                        }
                        RibbonButton { text: "Export SVG…"; tip: "Export an editable SVG of this view"; enabled: window.current !== null; onClicked: window.exportSvg() }
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
                        RibbonButton { text: "Keyboard shortcuts…"; glyph: "\uea65"; tip: "List all keyboard shortcuts"; enabled: true; onClicked: shortcuts.show("") }
                        RibbonButton { text: "About Basecraft…"; glyph: "\uea74"; tip: "Version and licence"; enabled: true; onClicked: about.open() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Online"
                        RibbonButton { text: "Project page"; glyph: "\uea84"; tip: "Open the project on GitHub"; enabled: true; onClicked: Qt.openUrlExternally("https://github.com/hverbruggen/basecraft") }
                        RibbonButton { text: "Report a problem"; glyph: "\ueaaf"; tip: "Open a new GitHub issue"; enabled: true; onClicked: Qt.openUrlExternally("https://github.com/hverbruggen/basecraft/issues/new") }
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
                        RibbonButton {
                            text: window.current && window.current.gap_column_count >= 0
                                  ? "Delete gap-only columns (" + window.current.gap_column_count + ")" : "Counting gap-only columns…"
                            enabled: window.current !== null && window.current.gap_column_count > 0
                            tip: "Delete columns containing only gaps across all rows, including hidden rows"
                            onClicked: window.current.remove_gap_columns()
                        }
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
                        RibbonButton {
                            text: "trimAl"
                            enabled: window.current !== null && !window.current.busy
                            tip: "Trim unreliable columns with trimAl automated1 into a new alignment"
                            onClicked: window.current.trim_alignment()
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
                        RibbonButton { text: "Delimitation…"; onClicked: { const path = window.current.create_delimitation(); if (path.length) window.openPath(path) } }
                        RibbonButton { text: "Annotate…"; tip: "Name the selected clade, with a bar or a coloured field"; keys: "Enter"; enabled: window.current !== null && window.current.has_selection === true; onClicked: cladeEditor.openSelected() }
                        RibbonButton { text: "Reroot"; tip: "Root the tree on the branch above the selection"; enabled: window.current !== null && window.current.has_selection === true; onClicked: window.current.reroot() }
                        RibbonButton { text: "Root position…"; tip: "Move the root along its branch"; onClicked: rootDialog.ask() }
                        RibbonButton { text: "Flip"; tip: "Reverse the order of the branches in the selected subtree (for a tip, its parent's)"; enabled: window.current !== null && window.current.has_selection === true; onClicked: window.current.rotate() }
                        RibbonButton { text: "Rename…"; tip: "Rename the selected tip"; keys: "F2"; enabled: window.current !== null && window.current.tip_selected === true; onClicked: tipDialog.ask() }
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
                        title: "Linked alignment"
                        RibbonButton { text: "Link alignment…"; tip: "Associate an imported tree with an alignment after validating its identifiers"; onClicked: linkAlignmentFile.open() }
                        RibbonButton { text: "Open selected clade(s)"; enabled: window.current !== null && !window.current.busy; tip: "Extract aligned rows without realigning; Ctrl-click to select multiple clades"; onClicked: window.current.extract_clade_alignment() }
                    }
                    RibbonGroup {
                        title: "Figure"
                        RibbonButton { text: "Export SVG…"; tip: "Save the complete tree as an editable SVG with the current colours and font"; onClicked: window.exportSvg() }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                // Build phylogeny: tree inference from the open alignment.
                RowLayout {
                    spacing: 6
                    RibbonGroup {
                        title: "basecraft-distance"
                        Repeater {
                            model: [{label: "Matrix", value: "matrix"}, {label: "NJ", value: "nj"}, {label: "UPGMA", value: "upgma"}]
                            delegate: RibbonButton {
                                required property var modelData
                                text: modelData.label
                                enabled: window.current !== null && !window.current.busy
                                tip: modelData.value === "matrix" ? "Save observed distances as TSV and a reusable cache" : modelData.value === "upgma" ? "Build a rooted, equal-rate UPGMA tree" : "Build a neighbor-joining tree"
                                onClicked: window.current.distance_action(modelData.value, JSON.stringify(distanceSettings.parameters()))
                            }
                        }
                        RibbonButton { text: "Settings…"; tip: "Distance model, overlap and workers"; onClicked: distanceSettings.open() }
                        ColumnLayout {
                            visible: window.currentKind === "alignment" && window.current !== null && window.current.bootstrap_running
                            Label { text: parent.visible ? window.current.bootstrap_status : ""; font.pixelSize: 11 }
                            ProgressBar { from: 0; to: 1; value: parent.visible ? window.current.bootstrap_progress : 0; Layout.fillWidth: true }
                            Button { text: "Cancel bootstrap"; enabled: parent.visible && !window.current.bootstrap_cancelling; onClicked: window.current.cancel_bootstrap() }
                        }
                    }
                    RibbonButton { text: "Delimitation…"; onClicked: { const path = window.current.create_delimitation(); if (path.length) window.openPath(path) } }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Maximum likelihood"
                        RibbonButton {
                            text: window.current && window.current.busy ? "Running…" : "FastTree"
                            enabled: window.current !== null && !window.current.busy
                            tip: "Infer a tree with FastTree, standard settings (GTR for DNA, LG for protein), into a new file"
                            onClicked: window.current.build_fasttree()
                        }
                        RibbonButton {
                            text: "IQ-TREE"
                            enabled: window.current !== null && !window.current.busy
                            tip: "Select a model and infer a maximum-likelihood tree; four workers, seed 1, no support resampling"
                            onClicked: window.current.build_iqtree()
                        }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }
                RowLayout {
                    Label { text: "Create, derive and edit hypotheses in the workspace below" }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

            }
            }
        }
    }

    Dialog {
        id: distanceSettings
        title: "Distance settings"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Close
        function parameters() {
            const p = {model: distanceModel.currentText, method: distanceMethod.currentText, min_overlap: distanceOverlap.value}
            if (distanceWorkers.value > 0) p.threads = distanceWorkers.value
            if (treeWorkers.value > 0) p.tree_threads = treeWorkers.value
            if (distanceBootstrap.checked) {
                p.bootstrap = bootstrapReplicates.value
                p.seed = bootstrapSeed.value
            }
            return p
        }
        GridLayout {
            columns: 2
            Label { text: "DNA model" }
            ComboBox { id: distanceModel; model: ["p", "jc69", "k2p"]; currentIndex: 2 }
            Label { text: "Minimum shared sites" }
            SpinBox { id: distanceOverlap; from: 1; to: 100000000; value: 1; editable: true }
            Label { text: "Distance workers (0 = auto)" }
            SpinBox { id: distanceWorkers; from: 0; to: 1024; value: 0; editable: true }
            Label { text: "NJ workers (0 = auto)" }
            SpinBox { id: treeWorkers; from: 0; to: 1024; value: 0; editable: true }
            Label { text: "NJ missing-distance method" }
            ComboBox { id: distanceMethod; model: ["njstar", "sorted", "fourpoint", "mvrstar"]; enabled: !distanceBootstrap.checked }
            CheckBox { id: distanceBootstrap; text: "Bootstrap support"; onCheckedChanged: if (checked) distanceMethod.currentIndex = 1 }
            SpinBox { id: bootstrapReplicates; from: 1; to: 1000000; value: 1000; editable: true; enabled: distanceBootstrap.checked }
            Label { text: "Bootstrap seed" }
            SpinBox { id: bootstrapSeed; from: 0; to: 2147483647; value: 1; editable: true; enabled: distanceBootstrap.checked }
            Label { Layout.columnSpan: 2; text: "Bootstrap uses complete sorted NJ or UPGMA; any failed replicate withholds support.\nMatrix export ignores bootstrap settings." }
            Label { Layout.columnSpan: 2; text: "UPGMA requires complete distances within each connected group.\nGaps and ambiguous residues are excluded pairwise." }
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
        id: genesDialog
        property bool inSelection: false
        function predict(selectionOnly) {
            inSelection = selectionOnly
            const code = window.current.genetic_code()
            let i = 0
            for (let k = 0; k < geneticCodes.count; k++)
                if (geneticCodes.get(k).code === code) i = k
            codeBox.currentIndex = i
            open()
        }
        title: inSelection ? "Predict genes in the selection" : "Predict genes"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: codeBox.forceActiveFocus()
        onAccepted: window.current.predict_genes(geneticCodes.get(codeBox.currentIndex).code, inSelection)
        ColumnLayout {
            spacing: 8
            RowLayout {
                Label { text: "Genetic code" }
                ComboBox {
                    id: codeBox
                    Layout.preferredWidth: 380
                    model: geneticCodes
                    textRole: "label"
                    Keys.onReturnPressed: genesDialog.accept()
                    Keys.onEnterPressed: genesDialog.accept()
                }
            }
            Label {
                text: "Prodigal trains on sequences of 20 kb or more. Shorter ones use its preset models,\nwhich exist only for codes 4 and 11. Genes are added as CDS; one Undo removes them all."
                opacity: 0.7
            }
        }
        // NCBI genetic codes that Prodigal supports.
        ListModel {
            id: geneticCodes
            ListElement { code: 1; label: "1  Standard" }
            ListElement { code: 2; label: "2  Vertebrate mitochondrial" }
            ListElement { code: 3; label: "3  Yeast mitochondrial" }
            ListElement { code: 4; label: "4  Mold, protozoan and coelenterate mitochondrial; Mycoplasma" }
            ListElement { code: 5; label: "5  Invertebrate mitochondrial" }
            ListElement { code: 6; label: "6  Ciliate, dasycladacean and Hexamita nuclear" }
            ListElement { code: 9; label: "9  Echinoderm and flatworm mitochondrial" }
            ListElement { code: 10; label: "10  Euplotid nuclear" }
            ListElement { code: 11; label: "11  Bacterial, archaeal and plant plastid" }
            ListElement { code: 12; label: "12  Alternative yeast nuclear" }
            ListElement { code: 13; label: "13  Ascidian mitochondrial" }
            ListElement { code: 14; label: "14  Alternative flatworm mitochondrial" }
            ListElement { code: 15; label: "15  Blepharisma nuclear" }
            ListElement { code: 16; label: "16  Chlorophycean mitochondrial" }
            ListElement { code: 21; label: "21  Trematode mitochondrial" }
            ListElement { code: 22; label: "22  Scenedesmus obliquus mitochondrial" }
            ListElement { code: 23; label: "23  Thraustochytrium mitochondrial" }
            ListElement { code: 24; label: "24  Rhabdopleuridae mitochondrial" }
            ListElement { code: 25; label: "25  Candidate division SR1 and Gracilibacteria" }
            ListElement { code: 26; label: "26  Pachysolen tannophilus nuclear" }
            ListElement { code: 29; label: "29  Mesodinium nuclear" }
            ListElement { code: 30; label: "30  Peritrich nuclear" }
            ListElement { code: 32; label: "32  Balanophoraceae plastid" }
            ListElement { code: 33; label: "33  Cephalodiscidae mitochondrial" }
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
    Shortcut { sequence: "Ctrl+Q"; onActivated: window.close() }
    // Close the active tab, asking first if it has unsaved changes.
    Shortcut { sequence: "Ctrl+W"; onActivated: if (openFiles.count > 0) window.closeTab(tabs.currentIndex) }
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
                                onOpenFile: (path) => window.openPath(path)
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                                onContextMenu: (info) => { treeMenu.info = info; treeMenu.popup() }
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                                onRenameTip: tipDialog.ask()
                                onAnnotate: cladeEditor.openSelected()
                            }
                        }
                        DelegateChoice {
                            roleValue: "delimitation"
                            DelimitationView {
                                onOpenFile: (path) => window.openPath(path)
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
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
        Item { text: "Predict genes in selection…"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: genesDialog.predict(true) }
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
        TreeItem { text: "Root position…"; visible: treeMenu.info.rootBranch === true; onTriggered: rootDialog.ask() }
        TreeItem { text: "Flip subtree"; visible: treeMenu.info.node; onTriggered: window.current.rotate() }
        TreeItem { text: "Rename tip…"; visible: treeMenu.info.tip; onTriggered: tipDialog.ask() }
        TreeItem { text: "Add support from tree…"; visible: !treeMenu.info.node; onTriggered: supportFile.open() }
        TreeItem { text: "Tree style…"; visible: !treeMenu.info.node; onTriggered: treeStyle.openStyle() }
    }

    // Where the root sits along its branch: a share of the branch on each
    // side. Halfway after rerooting.
    Dialog {
        id: rootDialog
        property var info: ({ side: "", other: "", length: 0, share: 0.5 })
        title: "Root position"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        function ask() {
            if (!window.treeOpen) return
            const i = JSON.parse(window.current.root_json())
            if (!i) {
                window.current.message_kind = "error"
                window.current.message = "The root needs two branches with lengths"
                return
            }
            info = i
            rootShare.value = i.share * 100
            open()
        }
        function length(share) { return (info.length * share).toPrecision(3) }
        onAccepted: window.current.move_root(rootShare.value / 100)
        onClosed: if (window.currentView) window.currentView.forceActiveFocus()
        ColumnLayout {
            spacing: 8
            Label { text: "Share of the root branch on each side"; opacity: 0.7 }
            RowLayout {
                Label { text: rootDialog.info.side; elide: Text.ElideMiddle; Layout.preferredWidth: 200; horizontalAlignment: Text.AlignRight }
                Slider { id: rootShare; from: 0; to: 100; stepSize: 0.5; Layout.preferredWidth: 220 }
                Label { text: rootDialog.info.other; elide: Text.ElideMiddle; Layout.preferredWidth: 200 }
            }
            RowLayout {
                Label {
                    text: rootShare.value.toFixed(1) + " % · " + rootDialog.length(rootShare.value / 100)
                    Layout.preferredWidth: 200
                    horizontalAlignment: Text.AlignRight
                }
                Button { text: "Halfway"; flat: true; Layout.preferredWidth: 220; onClicked: rootShare.value = 50 }
                Label { text: (100 - rootShare.value).toFixed(1) + " % · " + rootDialog.length(1 - rootShare.value / 100); Layout.preferredWidth: 200 }
            }
        }
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

    FileDialog {
        id: linkAlignmentFile
        title: "Link an alignment to this tree"
        nameFilters: ["FASTA alignments (*.fasta *.fa *.fas *.afa)"]
        onAccepted: if (window.treeOpen) window.current.link_alignment(window.localPath(selectedFile))
    }

    function exportSvg() {
        if (!window.current) return
        svgDialog.controller = window.current
        svgScope.currentIndex = window.treeOpen ? 2 : 0
        svgPublication.checked = false
        svgOptions.open()
    }
    Dialog {
        id: svgOptions
        title: "Export editable SVG"
        anchors.centerIn: parent
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        ColumnLayout {
            Label { text: "Region" }
            ComboBox { id: svgScope; model: ["Visible view", "Selection", "Complete figure"] }
            CheckBox { id: svgPublication; text: "Publication colours (white background)" }
        }
        onAccepted: {
            svgDialog.selectedFile = "file://" + window.currentPath.replace(/\.[^./]*$/, "") + ".svg"
            svgDialog.open()
        }
    }
    FileDialog {
        id: svgDialog
        property var controller: null
        title: "Export SVG"
        fileMode: FileDialog.SaveFile
        nameFilters: ["SVG figures (*.svg)"]
        onAccepted: if (controller) controller.export_figure(window.localPath(selectedFile), svgScope.currentIndex, svgPublication.checked)
    }

    // Quitting with unsaved changes: save them all, throw them away, or
    // stay.
    Dialog {
        id: quitDialog
        property var unsaved: []
        title: "Save changes before closing?"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Save | Dialog.Discard | Dialog.Cancel
        function ask(indexes) {
            unsaved = indexes
            quitError.text = ""
            open()
        }
        ColumnLayout {
            spacing: 6
            Label { text: quitDialog.unsaved.length === 1 ? "This file has unsaved changes:" : "These files have unsaved changes:" }
            Repeater {
                model: quitDialog.unsaved
                Label {
                    required property int modelData
                    text: "  " + window.fileName(openFiles.get(modelData).path)
                    font.bold: true
                }
            }
            Label { id: quitError; visible: text.length > 0; color: shell.accent; wrapMode: Text.Wrap }
        }
        onAccepted: {
            for (const i of unsaved) docs.itemAt(i).controller.save()
            // A save that failed leaves its tab unsaved: stay open and say so.
            const left = window.unsavedTabs()
            if (left.length > 0) {
                unsaved = left
                quitError.text = "Couldn't save " + left.map(i => window.fileName(openFiles.get(i).path)).join(", ")
                    + ": " + docs.itemAt(left[0]).controller.message
                open()
                return
            }
            window.closeConfirmed = true
            window.close()
        }
        onDiscarded: {
            close()
            window.closeConfirmed = true
            window.close()
        }
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
    // --no-sidebar 1  --fit 1  --aln STEPS  --keys KEYS  --shot-delay MS
    // --screenshot FILE
    Timer {
        interval: 900
        running: arg("--screenshot") !== undefined
        onTriggered: {
            const c = window.current
            const v = window.currentView
            if (!c) { console.log("no document open"); shot.start(); return }
            if (arg("--fit")) c.fit()
            if (arg("--delim")) {
                const steps = JSON.parse(arg("--delim"))
                for (const step of steps) {
                    if (step.action === "attach") c.attach_tree(step.path,step.rooted)
                    else if (step.action === "select") c.highlight_taxa(JSON.stringify(step.ids))
                    else if (step.action === "activate_last") { const work = JSON.parse(c.workspace_json); c.activate(work.hypotheses[work.hypotheses.length-1].id) }
                    else if (step.action === "cut") c.preview_cut(step.threshold)
                    else if (step.action === "cancel_cut") c.cancel_cut()
                    else if (step.action === "save_cut") c.save_cut(step.name)
                    else if (step.action === "undo") c.undo()
                    else if (step.action === "redo") c.redo()
                    else if (step.action === "save") c.save()
                    else if (step.action === "svg") c.export_svg(step.path)
                    else if (step.action === "visible_all") c.action("visible",JSON.stringify({ids:JSON.parse(c.workspace_json).hypotheses.map(h=>h.id)}))
                    else c.action(step.action,JSON.stringify(step))
                    console.log("delim",step.action,"|",c.message,"|",c.workspace_json,"| preview",c.preview_json)
                }
            }
            // Alignment edits, ';'-separated: sel:R0,C0,R1,C1  gap  rmgap
            // slide:N  type:TEXT  del  undo  redo  up  down  strip  zoomsel
            // insert  overwrite  mafft  fasttree
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
                    else if (cmd === "fasttree") c.build_fasttree()
                    else if (cmd === "trimal") c.trim_alignment()
                    else if (cmd === "iqtree") c.build_iqtree()
                    else if (cmd === "distance") c.build_distance_tree(a || "k2p")
                    console.log("aln", step, "|", c.message)
                }
            }
            // Key presses through the window, ';'-separated: ctrl+=  alt+-
            // ctrl+shift+=  enter  escape  (modifiers, then a character or
            // a key's name).
            if (arg("--keys")) {
                const mods = { ctrl: Qt.ControlModifier, alt: Qt.AltModifier, shift: Qt.ShiftModifier }
                for (const k of arg("--keys").split(";")) {
                    const parts = k.split("+")
                    const ch = parts.pop() || "+"
                    let m = 0
                    for (const p of parts) m |= mods[p] || 0
                    const named = { enter: [Qt.Key_Return, "\r"], escape: [Qt.Key_Escape, ""] }[ch]
                    if (named) v.pressKey(named[0], m, named[1])
                    else v.pressKey(ch.toUpperCase().charCodeAt(0), m, ch)
                    console.log("keys", k, "|", c.zoom_text, c.hscroll_size, c.vscroll_size)
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
            if (arg("--ribbon")) window.ribbonTabName = window.ribbonTabs[Number(arg("--ribbon"))]
            if (arg("--genes")) console.log(window.current.predict_genes(Number(arg("--genes")), arg("--select") !== undefined))
            if (arg("--orfs")) console.log(window.current.find_orfs(Number(arg("--orfs")), arg("--select") !== undefined))
            if (arg("--activity")) {
                const files = []
                for (let i = 0; i < openFiles.count; i++) files.push(openFiles.get(i).path)
                console.log("activity:", shell.activity_log(files.join("\n"), -new Date().getTimezoneOffset()))
            }
            if (arg("--distance-action")) c.distance_action(arg("--distance-action"), arg("--distance-options") || "{}")
            if (arg("--distance-settings")) distanceSettings.open()
            if (arg("--link-alignment")) c.link_alignment(arg("--link-alignment"))
            if (arg("--clade")) c.select_clade(JSON.stringify(arg("--clade").split(",")))
            if (arg("--export-svg")) c.export_figure(arg("--export-svg"), Number(arg("--export-scope") || 0), arg("--publication") === "1")
            if (arg("--extract-clade")) c.extract_clade_alignment()
            if (arg("--shortcuts")) shortcuts.show(window.ribbonTab)
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
