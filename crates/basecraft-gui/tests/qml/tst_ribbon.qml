import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtTest
import "../../qml"

Item {
    id: host
    width: 1600
    height: 600
    property int textSize: 12
    property bool busy: false
    property bool longLabels: false
    property bool rtl: false
    property bool dark: false
    property int invocations: 0
    AppCommand {
        id: command
        commandId: "test.run"
        text: host.longLabels ? "Run the selected analysis with a long translated label" : "Run"
        iconName: "accept"
        onTriggered: host.invocations++
    }
    Ribbon {
        id: ribbon
        width: 1000
        height: implicitHeight
        font.pixelSize: host.textSize
        palette.window: host.dark ? "#202020" : "#fafafa"
        palette.windowText: host.dark ? "#eeeeee" : "#202020"
        palette.buttonText: host.dark ? "#eeeeee" : "#202020"
        LayoutMirroring.enabled: host.rtl
        LayoutMirroring.childrenInherit: true
        RibbonPage {
            id: page
            RibbonGroup {
                id: actions
                title: "Actions"
                actions: [command]
            }
            ToolSeparator { Layout.fillHeight: true }
            RibbonGroup {
                id: fields
                title: "Compute allocation"
                Label { text: "Workers" }
                SpinBox { id: workers; from: 1; to: 128; editable: true }
            }
            RibbonGroup {
                id: progress
                title: "Job"
                AppRibbonButton { id: run; text: "Run"; tip: "Run analysis"; iconName: "accept" }
                ColumnLayout {
                    visible: host.busy
                    Label { text: "Calculating bootstrap support…" }
                    ProgressBar { value: 0.5 }
                    Button { text: "Cancel bootstrap" }
                }
            }
            Item { Layout.fillWidth: true }
            RibbonGroup {
                id: last
                title: "Help"
                AppRibbonButton { id: help; text: "Shortcuts"; tip: "Keyboard shortcuts" }
            }
        }
        RibbonPage {
            RibbonGroup { title: "Other page"; AppRibbonButton { text: "Open" } }
        }
    }
    TextInput { id: editor; x: 300; y: 350; width: 180; height: 30 }
    RibbonTabs {
        id: tabs
        y: 350
        width: 240
        names: ["File", "View", "Annotations", "Settings", "Help"]
        selectedName: "File"
        onSelected: name => selectedName = name
    }
    TestCase {
        name: "Ribbon"
        when: windowShown
        function init() {
            ribbon.currentIndex = 0
            ribbon.width = 1000
            host.textSize = 12
            host.busy = false
            host.longLabels = false
            host.rtl = false
            host.dark = false
            command.enabled = true
            failOnWarning(/Binding loop/)
            waitForPolish(ribbon)
        }
        function test_geometry_data() {
            const rows = []
            for (const width of [320, 640, 1200])
                for (const font of [12, 18, 24])
                    for (const busy of [false, true])
                        for (const rtl of [false, true])
                            for (const dark of [false, true])
                                rows.push({tag: `${width}-${font}-${busy}-${rtl}-${dark}`, width, font, busy, rtl, dark})
            return rows
        }
        function test_action_toolbar_is_compact() {
            verify(actions.implicitHeight < 50, "style background must not force 40-pixel toolbar buttons")
        }
        function test_geometry(data) {
            ribbon.width = data.width
            host.textSize = data.font
            host.busy = data.busy
            host.rtl = data.rtl
            host.dark = data.dark
            host.longLabels = true
            waitForPolish(ribbon)
            verify(page.height >= page.implicitHeight, "page must fit its natural height")
            for (const group of [actions, fields, progress, last]) {
                fuzzyCompare(group.height, actions.height, 1)
                fuzzyCompare(group.captionY, actions.captionY, 1)
                fuzzyCompare(group.bodyCenterY, actions.bodyCenterY, 1)
                verify(group.width >= group.implicitWidth, "no clipped controls/captions")
                const pos = group.mapToItem(ribbon, 0, 0)
                verify(pos.y + group.height <= ribbon.height, "caption below clip edge")
            }
            verify(workers.height >= workers.implicitHeight)
            verify(ribbon.contentWidth >= page.implicitWidth)
        }
        function test_keyboard_reveals_overflow_fields() {
            ribbon.width = 180
            waitForPolish(ribbon)
            verify(ribbon.overflowing)
            workers.forceActiveFocus(Qt.TabFocusReason)
            tryVerify(() => ribbon.scrollPosition > 0)
            tryVerify(() => {
                const position = workers.mapToItem(ribbon, 0, 0)
                return position.x >= 0 && position.x + workers.width <= ribbon.width
            }, 1000, "x=" + workers.mapToItem(ribbon, 0, 0).x + " width=" + workers.width + " scroll=" + ribbon.scrollPosition)
            help.forceActiveFocus(Qt.TabFocusReason)
            tryVerify(() => help.mapToItem(ribbon, 0, 0).x + help.width <= ribbon.width)
            compare(help.focusPolicy, Qt.TabFocus)
            workers.value = 1
            workers.forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Up)
            compare(workers.value, 2)
        }
        function test_page_change_resets_scroll_and_height() {
            host.busy = true
            ribbon.width = 180
            waitForPolish(ribbon)
            const tall = ribbon.height
            help.forceActiveFocus()
            tryVerify(() => ribbon.scrollPosition > 0)
            ribbon.currentIndex = 1
            waitForPolish(ribbon)
            compare(ribbon.scrollPosition, 0)
            verify(ribbon.height < tall)
            ribbon.currentIndex = -1
            waitForPolish(ribbon)
            compare(ribbon.currentPage, null)
        }
        function findActionButton(item, action) {
            if (item.action === action && item.visible) return item
            for (const child of item.children || []) {
                const found = findActionButton(child, action)
                if (found) return found
            }
            return null
        }
        function test_mouse_action_preserves_editor_focus() {
            editor.forceActiveFocus()
            mouseClick(run)
            verify(editor.activeFocus, "toolbar clicks must not interrupt typing in the editor")
            const button = findActionButton(actions, command)
            verify(button)
            compare(button.text, command.text)
            compare(button.display, AbstractButton.TextUnderIcon)
            const before = host.invocations
            mouseClick(button)
            compare(host.invocations, before + 1)
            verify(editor.activeFocus, "Kirigami actions must preserve mouse-driven editor focus too")
        }
        function test_tab_keyboard_changes_page() {
            tabs.selectedName = "File"
            waitForPolish(tabs)
            tabs.itemAt(0).forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Right)
            tryCompare(tabs, "selectedName", "View")
            keyClick(Qt.Key_End)
            // Explicit activation also works when the tab strip overflows.
            tabs.itemAt(4).forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Space)
            tryCompare(tabs, "selectedName", "Help")
            tabs.names = ["File", "View", "Alignment", "Build phylogeny", "Settings", "Help"]
            tryCompare(tabs, "currentIndex", 5)
        }
        function test_action_identity_and_disabled_state() {
            compare(command.objectName, "test.run")
            const before = host.invocations
            command.trigger()
            compare(host.invocations, before + 1)
            command.enabled = false
            command.trigger()
            compare(host.invocations, before + 1)
        }
    }
}
