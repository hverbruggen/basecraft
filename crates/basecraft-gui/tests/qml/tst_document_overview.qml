import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 600; height: 300
    Item { id: document; activeFocusOnTab: true }
    DocumentOverview {
        id: overview
        width: 500; height: implicitHeight
        viewportX: 0.1; viewportY: 0.2
        viewportWidth: 0.2; viewportHeight: 0.3
        documentItem: document
        property var pans: []
        onPanRequested: (x, y) => { pans = pans.concat([{x:x,y:y}]); viewportX=x; viewportY=y }
    }
    TestCase {
        name: "DocumentOverview"
        when: windowShown
        function init() {
            overview.shown = true
            overview.viewportWidth = 0.2; overview.viewportHeight = 0.3
            overview.viewportX = 0.1; overview.viewportY = 0.2
            overview.pans = []
        }
        function test_click_centres_and_clamps() {
            mouseClick(overview, overview.width - 3, overview.height - 3)
            verify(overview.pans.length > 0)
            compare(overview.viewportX, 0.8)
            compare(overview.viewportY, 0.7)
        }
        function test_marker_drag_preserves_grab_offset() {
            const marker = findChild(overview, "overviewViewport")
            const p = marker.mapToItem(overview, marker.width / 2, marker.height / 2)
            mousePress(overview, p.x, p.y)
            compare(overview.viewportX.toFixed(3), "0.100")
            mouseMove(overview, p.x + 50, p.y, 30)
            mouseRelease(overview, p.x + 50, p.y)
            verify(overview.viewportX > 0.19 && overview.viewportX < 0.21)
        }
        function test_keyboard_focus_and_no_refresh_theft() {
            overview.forceActiveFocus()
            keyClick(Qt.Key_Right)
            verify(overview.viewportX > 0.1)
            keyClick(Qt.Key_Down)
            verify(overview.viewportY > 0.2)
            keyClick(Qt.Key_Escape)
            verify(document.activeFocus)
            overview.miniature = '{"shapes":[{"x":0,"y":0,"w":1,"h":1,"color":"red","line":false}]}'
            verify(document.activeFocus)
        }
        function test_empty_fit_toggle_and_font() {
            overview.viewportWidth = 1; overview.viewportHeight = 1
            verify(!overview.visible)
            overview.viewportWidth = 0.5
            verify(overview.visible)
            overview.shown = false
            verify(!overview.visible)
            const before = overview.implicitHeight
            overview.font.pixelSize = 30
            verify(overview.implicitHeight >= 2 * overview.lineHeight)
            verify(overview.implicitHeight > before)
        }
        function test_optional_find_input_and_theme() {
            overview.matchMarks = [{x:0.4,y:0,width:0.01,height:1}]
            overview.foreground = "#fafafa"; overview.backgroundColor = "#171717"
            wait(50)
            compare(overview.matchMarks.length, 1)
            verify(overview.visible)
        }
    }
}
