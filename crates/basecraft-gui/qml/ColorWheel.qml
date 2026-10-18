import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Hue around the disc, saturation outward from the centre, brightness below.
ColumnLayout {
    id: wheel
    property color color: "#3d5a8a"
    signal edited(color color)
    spacing: 8
    // Hue and saturation survive dark colours, where `color` alone loses them.
    property real hue: Math.max(0, color.hsvHue)
    property real saturation: color.hsvSaturation
    property real value: color.hsvValue
    onColorChanged: {
        if (Qt.colorEqual(color, Qt.hsva(hue, saturation, value, 1))) return
        if (color.hsvHue >= 0 && color.hsvSaturation > 0 && color.hsvValue > 0) hue = color.hsvHue
        if (color.hsvValue > 0) saturation = color.hsvSaturation
        value = color.hsvValue
    }
    function pick(h, s, v) {
        hue = h; saturation = s; value = v
        color = Qt.hsva(h, s, v, 1)
        edited(color)
    }

    Item {
        id: disc
        Layout.preferredWidth: 180
        Layout.preferredHeight: 180
        Layout.alignment: Qt.AlignHCenter
        readonly property real radius: Math.min(width, height) / 2
        Canvas {
            anchors.fill: parent
            onPaint: {
                const ctx = getContext("2d")
                const r = disc.radius
                ctx.reset()
                const hues = ctx.createConicalGradient(r, r, 0)
                for (let i = 0; i <= 12; i++) hues.addColorStop(i / 12, Qt.hsva(i / 12, 1, 1, 1))
                ctx.beginPath()
                ctx.arc(r, r, r, 0, 2 * Math.PI)
                ctx.fillStyle = hues
                ctx.fill()
                const white = ctx.createRadialGradient(r, r, 0, r, r, r)
                white.addColorStop(0, "white")
                white.addColorStop(1, Qt.rgba(1, 1, 1, 0))
                ctx.fillStyle = white
                ctx.fill()
            }
        }
        // Darken the whole disc to the chosen brightness.
        Rectangle {
            anchors.fill: parent
            radius: disc.radius
            color: "black"
            opacity: 1 - wheel.value
        }
        Rectangle {
            readonly property real angle: wheel.hue * 2 * Math.PI
            readonly property real distance: wheel.saturation * disc.radius
            x: disc.radius + Math.cos(angle) * distance - width / 2
            y: disc.radius - Math.sin(angle) * distance - height / 2
            width: 12; height: 12; radius: 6
            color: "transparent"
            border.width: 2
            border.color: wheel.value > 0.5 ? "black" : "white"
        }
        MouseArea {
            anchors.fill: parent
            function choose(x, y) {
                const dx = x - disc.radius
                const dy = disc.radius - y
                const angle = Math.atan2(dy, dx)
                wheel.pick((angle < 0 ? angle + 2 * Math.PI : angle) / (2 * Math.PI),
                    Math.min(1, Math.sqrt(dx * dx + dy * dy) / disc.radius), wheel.value)
            }
            onPressed: event => choose(event.x, event.y)
            onPositionChanged: event => { if (pressed) choose(event.x, event.y) }
        }
        Accessible.role: Accessible.ColorChooser
        Accessible.name: "Hue and saturation"
    }
    RowLayout {
        Label { id: brightnessLabel; text: "Brightness" }
        Slider {
            objectName: "colorBrightness"
            Layout.fillWidth: true
            from: 0; to: 1
            value: wheel.value
            onMoved: wheel.pick(wheel.hue, wheel.saturation, value)
            Accessible.name: brightnessLabel.text
        }
    }
}
