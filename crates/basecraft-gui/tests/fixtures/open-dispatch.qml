import QtQuick
import Basecraft
Main {
    id: test
    visible: false
    property string fixtureFolder: OPEN_TEST_FOLDER
    property int step: 0
    property bool began: false
    property var identities: []
    property var paths: [fixtureFolder + "/alignment.afa", fixtureFolder + "/sequence.gb", fixtureFolder + "/tree.nwk"]
    property var kinds: ["alignment", "sequence", "tree"]
    function check(ok, message) {
        if (!ok) { console.error("OPEN_DISPATCH: " + message); Qt.exit(2) }
    }
    function settled() {
        if (!began || step >= paths.length || currentPath !== paths[step]) return
        check(currentKind === kinds[step], "wrong admitted kind " + currentKind)
        check(current && !current.dirty, "passive admission is dirty")
        check(documentAt(step).controller === current, "wrong slot/controller")
        identities.push(current)
        const same = current
        openPath(paths[step])
        check(current === same, "ready duplicate changed owner")
        check(documentAt(step + 1) === null, "ready duplicate created a tab")
        step++
        if (step < paths.length) {
            openPath(paths[step])
            openPath(paths[step]) // Pending duplicate must coalesce.
        } else {
            Qt.callLater(() => {
                for (let i = 0; i < paths.length; ++i)
                    check(documentAt(i).controller === identities[i], "another open replaced a ready owner")
                console.log("OPEN_DISPATCH: real three-view admission and duplicate checks passed")
                Qt.quit()
            })
        }
    }
    onCurrentChanged: Qt.callLater(settled)
    onCurrentPathChanged: Qt.callLater(settled)
    Component.onCompleted: Qt.callLater(() => {
        began = true
        openPath(paths[0])
        openPath(paths[0])
    })
    Timer {
        interval: 15000
        running: true
        onTriggered: { console.error("OPEN_DISPATCH: timed out at " + step + " / " + currentPath); Qt.exit(3) }
    }
}
