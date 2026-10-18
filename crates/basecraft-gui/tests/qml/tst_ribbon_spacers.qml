import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtTest
import "../../qml"

Item {
    width: 600; height: 200
    // A stretching spacer with a natural width of its own, as spacers report
    // in the app, must not make a page that fits overflow.
    Component {
        id: spacedRibbon
        Ribbon {
            width: 300
            RibbonPage {
                RibbonGroup { title: "One"; AppRibbonButton { text: "Run"; iconName: "accept" } }
                Item { Layout.fillWidth: true; implicitWidth: 200 }
                RibbonGroup { title: "Two"; AppRibbonButton { text: "Next"; iconName: "right" } }
            }
        }
    }
    TestCase {
        name: "RibbonSpacers"
        when: windowShown
        function test_stretching_spacers_never_cause_overflow() {
            const spaced = createTemporaryObject(spacedRibbon, parent)
            verify(waitForPolish(spaced))
            verify(spaced.pageWidth < 300)
            verify(!spaced.overflowing)
        }
    }
}
