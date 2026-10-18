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
            const replicates = findChild(dialog.body, "iqtreeReplicates")
            const seed = findChild(dialog.body, "iqtreeSeed")
            waitForPolish(replicates.parent)
            const before = [replicates.x, replicates.y, seed.x, seed.y]
            compare(replicates.width, seed.width, "The two number boxes match")
            support.currentIndex = 1
            support.activated(1)
            waitForPolish(replicates.parent)
            compare([replicates.x, replicates.y, seed.x, seed.y], before, "Choosing UFBoot keeps the fields in place")
            dialog.accept()
            const sent = JSON.parse(target.options)
            compare(sent.model, "GTR+I")
            compare(sent.support, "ufboot")
            compare(sent.replicates, 1000)
            compare(sent.seed, 1)
        }
    }
}
