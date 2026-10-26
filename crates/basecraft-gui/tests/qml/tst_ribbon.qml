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
    property real sizeFactor: 1
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
        baseFontPixelSize: host.textSize
        ribbonSizeFactor: host.sizeFactor
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
                AppRibbonButton { id: arrow; text: "Next"; tip: "Next item"; iconName: "right"; badge: "42" }
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
                AppRibbonButton { id: help; text: "Shortcuts…"; tip: "Keyboard shortcuts" }
            }
        }
        RibbonPage {
            RibbonGroup { title: "Other page"; AppRibbonButton { text: "Open" } }
        }
        RibbonPage {
            RibbonGroup { id: onlyActions; title: "Actions only"; actions: [command] }
        }
    }
    TextInput { id: editor; x: 300; y: 350; width: 180; height: 30 }
    RibbonTabs {
        id: tabs
        baseFontPixelSize: host.textSize
        ribbonSizeFactor: host.sizeFactor
        y: 350
        width: 240
        names: ["File", "View", "Annotations", "Settings", "Help"]
        selectedName: "File"
        onSelected: name => selectedName = name
    }
    Component {
        id: fractionalRibbon
        Ribbon {
            width: 320
            height: implicitHeight
            RibbonPage {
                RibbonGroup {
                    objectName: "fractionalGroup"
                    title: ""
                }
            }
        }
    }
    TestCase {
        name: "Ribbon"
        when: windowShown
        function init() {
            ribbon.currentIndex = 0
            ribbon.width = 1000
            host.textSize = 12
            host.sizeFactor = 1
            host.busy = false
            host.longLabels = false
            host.rtl = false
            host.dark = false
            tabs.names = ["File", "View", "Annotations", "Settings", "Help"]
            tabs.selectedName = "File"
            command.enabled = true
            failOnWarning(/Binding loop/)
            failOnWarning(/TypeError|ReferenceError/)
            // The Control itself has no pending polish; its nested layouts do.
            // Wait for the whole window before inspecting their geometry.
            verify(waitForPolish(ribbon.Window.window))
        }
        function test_geometry_data() {
            const rows = []
            for (const width of [320, 640, 1200])
                for (const font of [12, 18, 24])
                    for (const busy of [false, true])
                        for (const rtl of [false, true])
                            for (const dark of [false, true])
                                for (const factor of [1, 1.25, 1.5, 1.75, 2])
                                    rows.push({tag: `${width}-${font}-${busy}-${rtl}-${dark}-${factor}`, width, font, busy, rtl, dark, factor})
            return rows
        }
        function test_action_toolbar_is_compact() {
            verify(actions.implicitHeight < 60, "style background must not force 40-pixel toolbar buttons")
        }
        function test_caption_ellipsis_is_presentation_only() {
            compare(help.text, "Shortcuts…")
            compare(help.captionText, "Shortcuts")
            compare(help.Accessible.name, "Shortcuts…")
        }
        function test_action_only_page_fits_full_click_target_data() {
            const rows = []
            for (const factor of [1, 1.25, 1.5, 1.75, 2])
                for (const font of [12, 24])
                    rows.push({tag: `${factor}-${font}`, factor, font})
            return rows
        }
        function test_action_only_page_fits_full_click_target(data) {
            host.sizeFactor = data.factor
            host.textSize = data.font
            ribbon.currentIndex = 2
            ribbon.width = 320
            verify(waitForPolish(ribbon.Window.window))
            const button = findActionButton(onlyActions, command)
            verify(button)
            verify(button.height >= button.implicitHeight, "full tile must be clickable")
            const position = button.mapToItem(onlyActions, 0, 0)
            verify(position.y >= 0)
            verify(position.y + button.height <= onlyActions.captionY, "tile must fit above group caption")
            editor.forceActiveFocus()
            const before = host.invocations
            mouseClick(button, button.width / 2, button.height - 1)
            compare(host.invocations, before + 1, "caption/bottom padding belongs to the click target")
            verify(editor.activeFocus)
        }
        function test_live_scale_preserves_editor_font_and_restores_geometry() {
            const editorFont = editor.font
            const before = [run.width, run.height, run.captionBaseline, ribbon.height, tabs.height]
            help.forceActiveFocus(Qt.TabFocusReason)
            ribbon.width = 320
            host.sizeFactor = 2
            verify(waitForPolish(ribbon.Window.window))
            compare(run.icon.width, 32)
            verify(help.activeFocus, "resizing retains the focused command")
            tryVerify(() => help.mapToItem(ribbon, 0, 0).x + help.width <= ribbon.width)
            compare(editor.font, editorFont, "ribbon size must not change unrelated editor text")
            host.sizeFactor = 1
            ribbon.width = 1000
            verify(waitForPolish(ribbon.Window.window))
            compare([run.width, run.height, run.captionBaseline, ribbon.height, tabs.height], before)
        }
        function test_fractional_caption_width_data() {
            // Exercise both sides of half a pixel independently of installed fonts.
            return [{tag: "below-half", width: 32.21875}, {tag: "above-half", width: 32.78125}]
        }
        function test_fractional_caption_width(data) {
            const control = createTemporaryObject(fractionalRibbon, host)
            verify(control)
            const group = findChild(control, "fractionalGroup")
            const caption = group.children.find(child => child instanceof Label && child.text === group.title)
            verify(caption)
            // Fractional padding gives the empty caption a deterministic natural width.
            caption.leftPadding = data.width
            verify(waitForPolish(control.Window.window))
            compare(group.implicitWidth, 33, "group must request the next whole pixel")
            verify(group.width >= caption.implicitWidth, "group must reserve enough pixels for its caption")
            verify(caption.width >= caption.implicitWidth, "caption must retain its natural width")
        }
        function test_geometry(data) {
            ribbon.width = data.width
            host.textSize = data.font
            host.sizeFactor = data.factor
            host.busy = data.busy
            host.rtl = data.rtl
            host.dark = data.dark
            host.longLabels = true
            verify(waitForPolish(ribbon.Window.window))
            verify(page.height >= page.implicitHeight, "page must fit its natural height")
            for (const group of [actions, fields, progress, last]) {
                fuzzyCompare(group.height, actions.height, 1)
                fuzzyCompare(group.captionY, actions.captionY, 1)
                fuzzyCompare(group.bodyCenterY, actions.bodyCenterY, 1)
                verify(group.width >= group.implicitWidth,
                       `no clipped controls/captions: ${group.title} width=${group.width} implicitWidth=${group.implicitWidth} pageWidth=${page.width} pageImplicitWidth=${page.implicitWidth}`)
                const pos = group.mapToItem(ribbon, 0, 0)
                verify(pos.y + group.height <= ribbon.height, "caption below clip edge")
            }
            verify(workers.height >= workers.implicitHeight)
            compare(workers.font.pixelSize, Math.round(data.font * data.factor))
            compare(run.icon.width, 16 * data.factor)
            compare(arrow.icon.width, Math.round(10 * data.factor))
            compare(run.symbolHeight, arrow.symbolHeight, "arrows and counts use one symbol band")
            fuzzyCompare(run.captionBaseline, arrow.captionBaseline, 1)
            compare(run.font.pixelSize, Math.round(Math.max(9, Math.round(data.font * 0.8)) * data.factor))
            compare(tabs.itemAt(0).font.pixelSize, Math.round(data.font * data.factor))
            const button = findActionButton(actions, command)
            verify(button)
            compare(button.icon.width, 16 * data.factor, "ActionToolBar delegates inherit scale")
            fuzzyCompare(button.mapToItem(ribbon, 0, button.captionBaseline).y,
                         run.mapToItem(ribbon, 0, run.captionBaseline).y, 1,
                         "action delegates and direct buttons share the caption baseline")
            verify(button.height >= button.contentItem.implicitHeight + button.topPadding + button.bottomPadding,
                   "toolbar height=" + button.height + " natural=" + button.implicitHeight + " content=" + button.contentItem.implicitHeight + " group=" + actions.height)
            for (const control of [run, arrow, button]) {
                verify(control.width >= control.contentItem.implicitWidth + control.leftPadding + control.rightPadding)
                verify(control.height >= control.contentItem.implicitHeight + control.topPadding + control.bottomPadding)
            }
            verify(ribbon.contentWidth >= ribbon.pageWidth)
            fuzzyCompare(ribbon.pageWidth, page.implicitWidth, 1, "the summed width matches the page layout")
        }
        function test_keyboard_reveals_overflow_fields_data() {
            return [1, 1.25, 1.5, 1.75, 2].map(factor => ({tag: String(factor), factor}))
        }
        function test_keyboard_reveals_overflow_fields(data) {
            host.sizeFactor = data.factor
            ribbon.width = 180
            verify(waitForPolish(ribbon.Window.window))
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
            verify(waitForPolish(ribbon.Window.window))
            const tall = ribbon.height
            help.forceActiveFocus()
            tryVerify(() => ribbon.scrollPosition > 0)
            ribbon.currentIndex = 1
            verify(waitForPolish(ribbon.Window.window))
            compare(ribbon.scrollPosition, 0)
            verify(ribbon.height < tall)
            ribbon.currentIndex = -1
            verify(waitForPolish(ribbon.Window.window))
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
        function test_tab_keyboard_changes_page_data() {
            return [1, 1.25, 1.5, 1.75, 2].map(factor => ({tag: String(factor), factor}))
        }
        function test_tab_keyboard_changes_page(data) {
            host.sizeFactor = data.factor
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
            tabs.names = ["File", "View", "Alignment", "Phylogeny", "Settings", "Help"]
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
