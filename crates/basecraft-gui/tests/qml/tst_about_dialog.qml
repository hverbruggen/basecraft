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

    Component { id: spyComponent; SignalSpy {} }
    Component {
        id: dialogComponent
        // The application font is the desktop monospace family.
        AboutDialog { version: "0.0.0 (test)"; backgroundColor: "#f3eee4"; font.family: "monospace" }
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
            verify(findChild(dialog.body, "aboutCitation").text.length > 0)
            const copyright = findChild(dialog.body, "aboutCopyright")
            verify(copyright.text.indexOf("https://hverbruggen.codeberg.page/") >= 0)
            verify(copyright.activeFocusOnTab, "The lab link must be reachable by keyboard")
            for (const name of ["aboutShortcuts", "aboutProjectPage", "aboutReport", "aboutClose"])
                verify(findChild(dialog.footer, name).enabled, name + " must be usable")
            verify(!findChild(dialog.footer, "aboutLearn").enabled, "Learn waits for the manual pages")
            tryCompare(findChild(dialog.footer, "aboutClose"), "activeFocus", true)
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "visible", false)
            tryCompare(invoker, "activeFocus", true)
        }

        function test_shortcuts_button_hands_over() {
            const { dialog } = openDialog()
            const spy = createTemporaryObject(spyComponent, scene, { target: dialog, signalName: "shortcutsRequested" })
            mouseClick(findChild(dialog.footer, "aboutShortcuts"))
            compare(spy.count, 1)
            tryCompare(dialog, "visible", false)
        }

        function test_footer_wraps_in_narrow_window() {
            const { dialog } = openDialog()
            const close = findChild(dialog.footer, "aboutClose")
            waitForRendering(dialog.contentItem)
            // The buttons never set the dialog's width; the content does.
            verify(dialog.footer.implicitWidth < dialog.body.implicitWidth + dialog.leftPadding + dialog.rightPadding)
            dialog.width = 300
            waitForRendering(dialog.contentItem)
            verify(dialog.footer.height > close.height * 1.5, "Buttons must wrap onto more rows")
            verify(close.mapToItem(dialog.footer, close.width, 0).x <= dialog.footer.width, "Close must stay inside")
        }

        function test_art_follows_theme() {
            let { dialog, player } = openDialog({ backgroundColor: "#1c2630" })
            verify(player.dark)
            dialog.close()
            ;({ dialog, player } = openDialog({ backgroundColor: "#f3eee4" }))
            verify(!player.dark)
            compare(player.artPalette.background, "#f3eee4")
        }

        function test_every_animation_fits_and_plays() {
            const { dialog, player } = openDialog()
            verify(player.animationIndex >= 0 && player.animationIndex < player.animationCount)
            for (let i = 0; i < player.animationCount; ++i) {
                player.animationIndex = i
                const art = findChild(player, "asciiArt")
                tryVerify(() => Math.abs(player.width - player.animation.columns * player.cellWidth) < 1,
                          1000, player.animation.name + " must size the art to itself")
                tryVerify(() => player.width <= dialog.availableWidth + 0.5, 1000, player.animation.name + " must fit the dialog")
                const start = player.frame
                tryVerify(() => player.frame !== start, 2000, player.animation.name + " must advance")
                waitForRendering(player)
                // Coloured art, not just the dark panel, must reach the screen.
                const pixels = grabImage(player)
                let lit = 0
                for (let y = 0; y < pixels.height; y += 2)
                    for (let x = 0; x < pixels.width; x += 2)
                        if (pixels.red(x, y) + pixels.green(x, y) + pixels.blue(x, y) < 600) lit++
                verify(lit > 20, player.animation.name + " must draw visible characters on the light theme")
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
