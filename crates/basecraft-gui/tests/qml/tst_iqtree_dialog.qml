import QtQuick
import QtTest
import "../../qml"

Item {
    width: 800; height: 600
    Component { id: factory; IqTreeDialog {} }
    QtObject {
        id: target
        property bool busy: false
        property string options: ""
        function build_iqtree(json) { options = json }
    }

    TestCase {
        name: "IqTreeDialog"
        when: windowShown

        function test_model_and_support_reach_the_run() {
            const dialog = createTemporaryObject(factory, parent, {target: target})
            dialog.open()
            tryCompare(dialog, "opened", true)
            compare(dialog.model(), "MFP", "ModelFinder by default")
            verify(!findChild(dialog.body, "iqtreeGamma").parent.enabled, "Rate options follow the chosen model")
            findChild(dialog.body, "iqtreeModel").currentIndex = 8
            findChild(dialog.body, "iqtreeInvariant").checked = true
            compare(dialog.model(), "GTR+I+G")
            findChild(dialog.body, "iqtreeGamma").checked = false
            compare(dialog.model(), "GTR+I")
            compare(dialog.parameters().support, "none")
            verify(dialog.parameters().replicates === undefined)
            const support = findChild(dialog.body, "iqtreeSupportMethod")
            support.currentIndex = 1
            support.activated(1)
            dialog.accept()
            const sent = JSON.parse(target.options)
            compare(sent.model, "GTR+I")
            compare(sent.support, "ufboot")
            compare(sent.replicates, 1000)
            compare(sent.seed, 1)
        }
    }
}
