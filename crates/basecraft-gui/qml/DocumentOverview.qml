import QtQuick
import QtQuick.Controls.Basic

// One navigation surface for every scientific view. Fractions are document
// coordinates, never selection coordinates. Content is supplied by its owner.
Control {
    id: overview
    objectName: "documentOverview"
    property bool shown: true
    property bool vertical: false
    property bool circular: false
    property real viewportX: 0
    property real viewportY: 0
    property real viewportWidth: 1
    property real viewportHeight: 1
    property string miniature: "{\"shapes\":[]}"
    // Optional Find adapter input: normalized {x,y,width,height} rectangles.
    property var matchMarks: []
    property Item documentItem: null
    property color foreground: palette.text
    property color backgroundColor: palette.base
    property color accent: palette.highlight
    property real lineHeight: metrics.height
    readonly property bool overflowing: viewportWidth < 0.999 || viewportHeight < 0.999
    readonly property var content: JSON.parse(miniature)
    signal panRequested(real x, real y)
    visible: shown && overflowing
    implicitHeight: Math.max(24, 2 * lineHeight)
    implicitWidth: Math.max(24, 3 * lineHeight)
    focusPolicy: Qt.StrongFocus
    activeFocusOnTab: visible
    padding: 2
    Accessible.name: qsTr("Document overview")
    Accessible.description: qsTr("Click to centre the view. Arrow keys pan; Escape returns to the document.")
    Accessible.role: Accessible.Pane
    FontMetrics { id: metrics; font: overview.font }
    function bounded(value, span) { return Math.max(0, Math.min(1 - Math.max(0, Math.min(1, span)), value)) }
    function pan(x, y) {
        if (isFinite(x) && isFinite(y)) panRequested(bounded(x, viewportWidth), bounded(y, viewportHeight))
    }
    onContentChanged: drawing.requestPaint()
    onMatchMarksChanged: drawing.requestPaint()
    onCircularChanged: drawing.requestPaint()
    onForegroundChanged: drawing.requestPaint()
    onBackgroundColorChanged: drawing.requestPaint()
    background: Rectangle {
        color: overview.backgroundColor
        border.width: overview.activeFocus ? 2 : 0
        border.color: overview.accent
    }
    contentItem: Item {
        id: surface
        Canvas {
            id: drawing
            anchors.fill: parent
            opacity: 0.6
            onPaint: {
                const c = getContext("2d")
                c.clearRect(0, 0, width, height)
                for (const mark of overview.content.shapes || []) {
                    c.fillStyle = mark.color
                    c.strokeStyle = mark.color
                    c.lineWidth = 1
                    if (overview.circular) {
                        const r = Math.min(width, height) * (0.22 + mark.y * 0.23)
                        c.beginPath()
                        c.arc(width / 2, height / 2, r, mark.x * 2 * Math.PI - Math.PI / 2,
                              (mark.x + mark.w) * 2 * Math.PI - Math.PI / 2)
                        c.lineWidth = Math.max(1, mark.h * Math.min(width, height) * 0.23)
                        c.stroke()
                    } else if (mark.line) {
                        c.beginPath(); c.moveTo(mark.x * width, mark.y * height)
                        c.lineTo(mark.w * width, mark.h * height); c.stroke()
                    } else c.fillRect(mark.x * width, mark.y * height, Math.max(1, mark.w * width), Math.max(1, mark.h * height))
                }
                c.fillStyle = overview.accent
                for (const mark of overview.matchMarks) {
                    if (isFinite(mark.x) && isFinite(mark.y))
                        c.fillRect(mark.x * width, mark.y * height,
                                   Math.max(2, (mark.width || 0) * width), Math.max(2, (mark.height || 0) * height))
                }
            }
        }
        Rectangle {
            id: marker
            objectName: "overviewViewport"
            x: overview.bounded(overview.viewportX, overview.viewportWidth) * surface.width
            y: overview.bounded(overview.viewportY, overview.viewportHeight) * surface.height
            width: Math.max(2, Math.min(1, overview.viewportWidth) * surface.width)
            height: Math.max(2, Math.min(1, overview.viewportHeight) * surface.height)
            color: "transparent"
            border.width: 2
            border.color: overview.foreground
            // A contrasting inner edge keeps the viewport legible over data.
            Rectangle { anchors.fill: parent; anchors.margins: 2; color: "transparent"; border.width: 1; border.color: overview.backgroundColor }
        }
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton
            property real offsetX: 0
            property real offsetY: 0
            onPressed: mouse => {
                overview.forceActiveFocus(Qt.MouseFocusReason)
                const inside = mouse.x >= marker.x && mouse.x <= marker.x + marker.width
                    && mouse.y >= marker.y && mouse.y <= marker.y + marker.height
                offsetX = inside ? mouse.x / Math.max(1, width) - overview.viewportX : overview.viewportWidth / 2
                offsetY = inside ? mouse.y / Math.max(1, height) - overview.viewportY : overview.viewportHeight / 2
                overview.pan(mouse.x / Math.max(1, width) - offsetX, mouse.y / Math.max(1, height) - offsetY)
            }
            onPositionChanged: mouse => {
                if (pressed) overview.pan(mouse.x / Math.max(1, width) - offsetX, mouse.y / Math.max(1, height) - offsetY)
            }
        }
    }
    Keys.onPressed: event => {
        const dx = Math.max(0.005, viewportWidth / 10)
        const dy = Math.max(0.005, viewportHeight / 10)
        if (event.key === Qt.Key_Escape && documentItem) documentItem.forceActiveFocus(Qt.ShortcutFocusReason)
        else if (event.key === Qt.Key_Left) pan(viewportX - dx, viewportY)
        else if (event.key === Qt.Key_Right) pan(viewportX + dx, viewportY)
        else if (event.key === Qt.Key_Up) pan(viewportX, viewportY - dy)
        else if (event.key === Qt.Key_Down) pan(viewportX, viewportY + dy)
        else { event.accepted = false; return }
        event.accepted = true
    }
    onVisibleChanged: if (!visible && activeFocus && documentItem) documentItem.forceActiveFocus(Qt.OtherFocusReason)
}
