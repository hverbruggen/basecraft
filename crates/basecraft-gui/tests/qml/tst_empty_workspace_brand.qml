import QtQuick
import QtQuick.Controls
import QtTest
import "../../qml"

Item {
    id: scene
    width: 1000
    height: 600
    Rectangle { anchors.fill: parent; color: brand.backgroundColor }
    EmptyWorkspaceBrand { id: brand; anchors.fill: parent; backgroundColor: "#1c2630" }
    Button { id: focusTarget; text: "Open"; x: 8; y: 8 }

    TestCase {
        name: "EmptyWorkspaceBrand"
        when: windowShown

        function test_themes_and_resizing() {
            const logo = findChild(brand, "emptyWorkspaceLogo")
            const subtitle = findChild(brand, "emptyWorkspaceSubtitle")
            verify(logo !== null && subtitle !== null)
            focusTarget.forceActiveFocus()
            for (const bg of ["#1c2630", "#f3eee4"]) {
                brand.backgroundColor = bg
                tryCompare(logo, "status", Image.Ready)
                for (const size of [[1000, 600], [640, 480], [1600, 900]]) {
                    scene.width = size[0]
                    scene.height = size[1]
                    waitForRendering(brand)
                    const point = logo.mapToItem(scene, logo.width / 2, logo.height / 2)
                    fuzzyCompare(point.x, scene.width / 2, 1)
                    verify(logo.width > 0 && logo.width <= scene.width)
                    verify(subtitle.mapToItem(scene, 0, subtitle.height).y <= scene.height)
                    verify(subtitle.text.indexOf("provenance built in") >= 0)
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
