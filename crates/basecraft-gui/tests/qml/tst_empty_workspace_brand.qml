import QtQuick
import QtQuick.Controls
import QtTest
import "../../qml"

Item {
    id: scene
    width: 1000
    height: 600
    property real mainPanelWidth: width
    Rectangle { anchors.fill: parent; color: brand.backgroundColor }
    EmptyWorkspaceBrand { id: brand; anchors.left: parent.left; anchors.top: parent.top; anchors.bottom: parent.bottom; width: scene.mainPanelWidth; backgroundColor: "#1c2630" }
    SignalSpy { id: openSpy; target: brand; signalName: "openProject" }
    SignalSpy { id: recentSpy; target: brand; signalName: "openRecent" }
    Button { id: focusTarget; text: "Open"; x: 8; y: 8 }

    TestCase {
        name: "EmptyWorkspaceBrand"
        when: windowShown

        function test_project_actions() {
            brand.recentProjects = ["/project/one", "/missing/project"]
            const open = findChild(brand, "startOpenProject")
            open.forceActiveFocus()
            keyClick(Qt.Key_Space)
            compare(openSpy.count, 1)
            const recent = findChild(brand, "startRecentProjects")
            tryCompare(recent, "count", 2)
            recent.forceLayout()
            tryVerify(() => recent.itemAtIndex(1) !== null)
            recent.itemAtIndex(1).clicked()
            compare(recentSpy.signalArguments[0][0], "/missing/project")
            brand.recentProjects = []
        }
        function test_themes_and_resizing() {
            const logo = findChild(brand, "emptyWorkspaceLogo")
            verify(logo !== null)
            focusTarget.forceActiveFocus()
            for (const bg of ["#1c2630", "#f3eee4"]) {
                brand.backgroundColor = bg
                tryCompare(logo, "status", Image.Ready)
                for (const size of [[1000, 600], [640, 480], [1600, 900], [800, 600], [800, 1000]]) {
                    scene.width = size[0]
                    scene.height = size[1]
                    mainPanelWidth = size[0] === 800 ? 400 : size[0]
                    waitForRendering(brand)
                    const point = logo.mapToItem(brand, logo.width / 2, logo.height / 2)
                    fuzzyCompare(point.x, brand.width / 2, 1)
                    verify(point.y > 0 && point.y < brand.height)
                    fuzzyCompare(logo.width / logo.height, 1020 / 420, 0.001)
                    verify(logo.width > 0 && logo.width <= scene.width)
                    fuzzyCompare(logo.width, brand.width * 0.25, 0.001)
                    verify(logo.mapToItem(brand, 0, logo.height).y <= brand.height)
                    compare(logo.Accessible.name, "Basecraft")
                    verify(focusTarget.activeFocus, "Decorative mark must not steal focus")
                    // Verify the actual coloured mark survives image loading and compositing.
                    const pixels = grabImage(scene)
                    let colored = 0
                    for (let y = 0; y < pixels.height; y += 2)
                        for (let x = 0; x < pixels.width; x += 2)
                            if (pixels.red(x, y) > pixels.blue(x, y) + 20
                                && pixels.green(x, y) > pixels.blue(x, y) + 10) colored++
                    verify(colored > 10, "Amber paper must render in both themes")
                }
            }
        }
    }
}
