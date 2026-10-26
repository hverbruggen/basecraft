import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: scene
    width: 1000
    height: 760
    Rectangle { anchors.fill: parent; color: "#f3eee4" }
    Button { id: invoker; text: "About"; x: 8; y: 8 }

    Component {
        id: dialogComponent
        AboutDialog { version: "0.0.0 (test)"; backgroundColor: "#f3eee4" }
    }

    TestCase {
        name: "AboutDialog"
        when: windowShown

        function openDialog(props) {
            invoker.forceActiveFocus()
            const dialog = createTemporaryObject(dialogComponent, scene, props || {})
            dialog.open()
            tryCompare(dialog, "opened", true)
            const loader = findChild(dialog.body, "aboutArt")
            tryCompare(loader, "status", Loader.Ready)
            return { dialog: dialog, player: loader.item }
        }

        function test_content() {
            const { dialog } = openDialog()
            tryCompare(findChild(dialog.body, "aboutLogo"), "status", Image.Ready)
            compare(findChild(dialog.body, "aboutVersion").text, "0.0.0 (test)")
            verify(findChild(dialog.body, "aboutTagline").text.indexOf("provenance built in") >= 0)
            verify(findChild(dialog.body, "aboutCitation").text.length > 0)
            compare(findChild(dialog.body, "aboutUrl").text, "https://github.com/hverbruggen/basecraft")
            const button = findChild(dialog.body, "aboutProjectPage")
            verify(button.activeFocusOnTab, "The project page must be reachable by keyboard")
            // Focus lands inside the dialog and returns to the invoker on Escape.
            tryVerify(() => dialog.activeFocus || findChild(dialog.body, "aboutProjectPage").activeFocus
                || dialog.footer.activeFocus || dialog.contentItem.activeFocus)
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "visible", false)
            tryCompare(invoker, "activeFocus", true)
        }

        function test_every_animation_fits_and_plays() {
            const { dialog, player } = openDialog()
            verify(player.animationIndex >= 0 && player.animationIndex < player.animationCount)
            const width = player.width, height = player.height
            for (let i = 0; i < player.animationCount; ++i) {
                player.animationIndex = i
                const art = findChild(player, "asciiArt")
                verify(art.width <= player.width && art.height <= player.height,
                       player.animation.name + " must fit its panel")
                fuzzyCompare(player.width, width, 1, "Switching animations must not resize the dialog")
                fuzzyCompare(player.height, height, 1)
                const start = player.frame
                tryVerify(() => player.frame !== start, 2000, player.animation.name + " must advance")
                waitForRendering(player)
                // Coloured art, not just the dark panel, must reach the screen.
                const pixels = grabImage(player)
                let lit = 0
                for (let y = 0; y < pixels.height; y += 2)
                    for (let x = 0; x < pixels.width; x += 2)
                        if (pixels.red(x, y) + pixels.green(x, y) + pixels.blue(x, y) > 300) lit++
                verify(lit > 20, player.animation.name + " must draw visible characters")
            }
        }

        function test_bounded_by_small_window() {
            scene.width = 640
            scene.height = 480
            const { dialog } = openDialog()
            verify(dialog.width <= scene.width && dialog.height <= scene.height)
            scene.width = 1000
            scene.height = 760
        }

        function test_dark_background_uses_dark_wordmark() {
            const { dialog } = openDialog({ backgroundColor: "#1c2630" })
            const logo = findChild(dialog.body, "aboutLogo")
            verify(logo.darkBackground)
            verify(String(logo.source).indexOf("wordmark-dark") >= 0)
        }

        function test_art_stops_when_closed() {
            const { dialog } = openDialog()
            dialog.close()
            tryCompare(dialog, "visible", false)
            tryCompare(findChild(dialog.body, "aboutArt"), "item", null)
        }
    }
}
