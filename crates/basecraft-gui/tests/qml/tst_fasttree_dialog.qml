import QtQuick
import QtTest
import "../../qml"

Item {
    width: 800; height: 600
    Component { id: factory; FastTreeDialog {} }
    QtObject {
        id: target
        property bool busy: false
        property string options: ""
        function build_fasttree(json) { options = json }
    }

    TestCase {
        name: "FastTreeDialog"
        when: windowShown

        function test_defaults_are_the_standard_run() {
            const dialog = createTemporaryObject(factory, parent, {target: target})
            dialog.open()
            tryCompare(dialog, "opened", true)
            compare(JSON.stringify(dialog.parameters()), JSON.stringify({gamma: false, support: true, resamples: 1000}))
            findChild(dialog.body, "fasttreeModel").currentIndex = 1
            findChild(dialog.body, "fasttreeGamma").checked = true
            findChild(dialog.body, "fasttreeSupport").checked = false
            dialog.accept()
            const sent = JSON.parse(target.options)
            compare(sent.model, "JC")
            compare(sent.gamma, true)
            compare(sent.support, false)
            verify(sent.resamples === undefined)
        }
    }
}
