import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtTest
import "../../qml"
import "tool_forms.js" as Forms

Item {
    id: host
    width: 1100; height: 800
    Component {
        id: windowComponent
        ApplicationWindow {
            id: window
            width: 1000; height: 760
            visible: true
            font.family: "monospace"
            font.pixelSize: 12
            property alias distance: distance
            property alias delimitation: delimitation
            property alias confirmation: confirmation
            TextField { id: editor; objectName: "editor"; text: "Document" }
            ToolSettingsDialog { id: distance }
            DelimitationAnalysisDialog { id: delimitation }
            AppDialog {
                id: confirmation
                title: "Save changes?"
                standardButtons: Dialog.Save | Dialog.Discard | Dialog.Cancel
            }
        }
    }
    TestCase {
        name: "AnalysisDialogGeometry"
        when: windowShown
        function init() { failOnWarning(/Binding loop|Cannot assign|Unable to assign|Both point size/) }
        function cases(kinds) {
            const cases = []
            for (const kind of kinds)
                for (const size of [12, 24])
                    for (const width of [360, 1000])
                        cases.push({tag: `${kind}-${size}-${width}`, kind, size, width})
            return cases
        }
        function makeWindow(data) {
            const window = createTemporaryObject(windowComponent, host, {width: data.width})
            verify(window)
            window.font.pixelSize = data.size
            window.requestActivate()
            tryCompare(window, "active", true)
            return window
        }
        function inside(item, container, message) {
            const p = item.mapToItem(container, 0, 0)
            tryVerify(() => {
                const p = item.mapToItem(container, 0, 0)
                return p.x >= 0 && p.x + item.width <= container.width
            }, 1000, message + `: x=${p.x}, width=${item.width}, available=${container.width}`)
        }
        function checkFrame(dialog, window) {
            wait(0)
            waitForPolish(window.contentItem)
            verify(dialog.width <= window.width - 32)
            verify(dialog.height <= window.height - 32)
            compare(dialog.header.font.pixelSize, window.font.pixelSize)
            verify(dialog.background.border.width >= 1)
            compare(dialog.background.width, dialog.width)
            compare(dialog.background.height, dialog.height)
            const body = dialog.contentItem.mapToItem(dialog.background, 0, 0)
            verify(body.x >= 1 && body.y >= 1)
            verify(body.x + dialog.contentItem.width <= dialog.width - 1)
            verify(body.y + dialog.contentItem.height <= dialog.height - 1)
            const footer = dialog.footer.mapToItem(dialog.background, 0, 0)
            tryVerify(() => dialog.footer.mapToItem(dialog.background, 0, 0).y >=
                body.y + dialog.contentItem.height)
            verify(footer.y + dialog.footer.height <= dialog.height)
            for (const role of [Dialog.Ok, Dialog.Cancel]) {
                const button = dialog.standardButton(role)
                compare(button.font.pixelSize, window.font.pixelSize)
                verify(button.width <= button.implicitWidth + 1, "Footer buttons keep their natural width")
                inside(button, dialog.background, "Footer button stays inside the border")
            }
        }
        function checkField(dialog, name, fontSize) {
            wait(0)
            waitForPolish(dialog.contentItem)
            const field = findChild(dialog.contentItem, name)
            verify(field, name)
            compare(field.font.pixelSize, fontSize)
            verify(field.Accessible.name.length > 0, name + " has an accessible name")
            if (field.Accessible.labelledBy) compare(field.Accessible.labelledBy.font.pixelSize, fontSize)
            inside(field, dialog.contentItem, name)
            return field
        }
        function reveal(dialog, field) {
            field.forceActiveFocus(Qt.TabFocusReason)
            tryVerify(() => {
                const p = field.mapToItem(dialog.contentItem, 0, 0)
                return p.y >= -1 && p.y + field.height <= dialog.contentItem.height + 1
            }, 1000, field.objectName + " is revealed by keyboard focus")
        }
        function field(dialog, name) { return findChild(dialog.contentItem, name) }
        function test_distance_data() { return cases(["matrix", "nj", "upgma"]) }
        function test_distance(data) {
            const window = makeWindow(data)
            const dialog = window.distance
            const editor = findChild(window.contentItem, "editor")
            editor.forceActiveFocus()
            tryCompare(editor, "activeFocus", true)
            dialog.show(Forms.target(), "distance", data.kind)
            tryCompare(dialog, "opened", true)
            const model = checkField(dialog, "model", data.size)
            tryCompare(model, "activeFocus", true)
            checkField(dialog, "min_overlap", data.size)
            tryCompare(field(dialog, "field-model"), "columns", data.width === 360 && data.size === 24 ? 1 : 2)
            checkFrame(dialog, window)
            if (data.kind !== "matrix") {
                const bootstrap = checkField(dialog, "bootstrap_on", data.size)
                verify(bootstrap.indicator.width <= Math.max(16, bootstrap.font.pixelSize * 1.5))
                reveal(dialog, bootstrap)
                keyClick(Qt.Key_Space)
                verify(bootstrap.checked)
                const replicates = checkField(dialog, "bootstrap", data.size)
                const seed = checkField(dialog, "seed", data.size)
                reveal(dialog, replicates)
                keyClick(Qt.Key_Up)
                compare(dialog.values.bootstrap, 1001)
                reveal(dialog, seed)
                keyClick(Qt.Key_Up)
                compare(dialog.values.seed, 2)
                const destination = checkField(dialog, "bootstrap_destination", data.size)
                reveal(dialog, destination)
                keyClick(Qt.Key_Down)
                compare(dialog.values.bootstrap_destination, "working")
                seed.value = seed.to
                waitForPolish(dialog.contentItem)
                tryVerify(() => seed.contentItem.width >= seed.contentItem.contentWidth,
                    1000, "The largest seed fits without clipping")
                for (let i = 0; i < 10; ++i) {
                    keyClick(Qt.Key_Tab)
                    verify(!editor.activeFocus, "Focus stays inside the modal dialog")
                }
            }
            dialog.reject()
            tryCompare(editor, "activeFocus", true)
        }
        function test_delimitation_data() { return cases(["gmyc", "asap", "ptp", "manual", "view"]) }
        function test_delimitation(data) {
            const window = makeWindow(data)
            const dialog = window.delimitation
            dialog.show(data.kind, "/project/input.afa", "/project/input.nwk")
            tryCompare(dialog, "opened", true)
            checkFrame(dialog, window)
            if (data.kind === "gmyc") {
                const model = checkField(dialog, "gmycThresholds", data.size)
                checkField(dialog, "deduplicateInput", data.size)
                const row = findChild(dialog, "samplingSettings")
                tryCompare(row, "columns", data.width === 360 ? 1 : 2)
                model.currentIndex = 1
                const budget = checkField(dialog, "candidateLimit", data.size)
                reveal(dialog, budget)
                keyClick(Qt.Key_Up)
                compare(dialog.parameters().candidate_budget, 1001)
                compare(dialog.parameters().deduplicate, true)
                checkField(dialog, "distanceModel", data.size)
                checkField(dialog, "distanceOverlap", data.size)
                tryCompare(findChild(dialog, "distanceSettings"), "columns",
                    data.width === 360 && data.size === 24 ? 1 : 2)
            }
            if (data.kind === "manual" || data.kind === "view") {
                findChild(dialog, "manualTree").currentIndex = 4
                verify(!findChild(dialog, "deduplicateInput").visible)
            } else if (data.kind === "asap") {
                findChild(dialog, "asapView").currentIndex = 3
            }
            if (dialog.iqtreeDisplay) {
                const model = checkField(dialog, "iqtreeModel", data.size)
                reveal(dialog, model)
                model.text = "GTR+G"
                compare(dialog.parameters().iqtree_model, "GTR+G")
                const seed = checkField(dialog, "randomSeed", data.size)
                reveal(dialog, seed)
            }
            dialog.reject()
        }
        function test_live_font_and_width_changes() {
            const window = makeWindow({width: 1000, size: 12})
            const dialog = window.distance
            dialog.show(Forms.target(), "distance", "nj")
            tryCompare(dialog, "opened", true)
            const row = field(dialog, "field-bootstrap_destination")
            tryCompare(row, "columns", 2)
            dialog.set("bootstrap_on", true)
            dialog.set("seed", 12345)
            const before = JSON.stringify(dialog.values)
            window.width = 360
            window.font.pixelSize = 24
            tryCompare(row, "columns", 1)
            checkField(dialog, "seed", 24)
            compare(JSON.stringify(dialog.values), before)
            reveal(dialog, field(dialog, "seed"))
            window.width = 1000
            tryCompare(row, "columns", 2)
            checkFrame(dialog, window)
            compare(JSON.stringify(dialog.values), before)
            dialog.reject()
        }
        function test_narrow_confirmation_buttons() {
            const window = makeWindow({width: 360, size: 24})
            const dialog = window.confirmation
            dialog.open()
            tryCompare(dialog, "opened", true)
            tryCompare(dialog.footer, "stacked", true)
            const cancel = dialog.standardButton(Dialog.Cancel)
            tryCompare(cancel, "activeFocus", true)
            for (const role of [Dialog.Save, Dialog.Discard, Dialog.Cancel]) {
                const button = dialog.standardButton(role)
                inside(button, dialog.background, "Confirmation button stays inside the border")
                tryVerify(() => {
                    const p = button.mapToItem(dialog.background, 0, 0)
                    return p.y >= 1 && p.y + button.height <= dialog.height - 1
                }, 1000, "Stacked buttons stay inside the top and bottom border")
                button.forceActiveFocus(Qt.TabFocusReason)
                verify(button.activeFocus)
            }
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
        }
    }
}
