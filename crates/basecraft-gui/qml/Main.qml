import QtQuick
import QtQuick.Window
import Basecraft

Window {
    id: window
    width: 900
    height: 600
    visible: true
    title: "Basecraft"
    color: "#f0e6d3"

    Hello {
        id: hello
    }

    InstanceView {
        id: view
        anchors.fill: parent
        clearColor: "#f0e6d3"
        instances: hello.frame
        onWidthChanged: hello.layout(width, height)
        onHeightChanged: hello.layout(width, height)
    }

    // Measurement hooks for scripted runs: BASECRAFT_SCREENSHOT=<png> saves
    // the window after the first frames and quits.
    Timer {
        interval: 800
        running: Qt.application.arguments.indexOf("--screenshot") >= 0
        onTriggered: {
            const args = Qt.application.arguments
            const path = args[args.indexOf("--screenshot") + 1]
            console.log(hello.memory_report(), "|", hello.status)
            console.log("saved", view.saveWindow(path))
            Qt.quit()
        }
    }
}
