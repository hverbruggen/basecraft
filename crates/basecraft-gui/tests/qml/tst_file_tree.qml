import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 480
    height: 240
    property string openedPath: ""
    ListModel {
        id: files
        function isDirAt(index) { return false }
        // Deliberately opaque, like QFileSystemModel's invokable lookup.
        property var lookupPaths: []
        function pathAt(index) { return lookupPaths[index.row] }
    }
    TreeView {
        id: tree
        anchors.fill: parent
        model: files
        delegate: FileTreeDelegate {
            currentPath: "/project/zebra.afa"
            onOpenRequested: path => openedPath = path
        }
    }
    TestCase {
        name: "FileTreeIdentity"
        when: windowShown
        function init() {
            files.lookupPaths[0] = "/project/alpha.afa"
            files.lookupPaths[1] = "/project/zebra.afa"
            files.clear()
            files.append({display: "alpha.afa", filePath: "/project/alpha.afa"})
            files.append({display: "zebra.afa", filePath: "/project/zebra.afa"})
            openedPath = ""
            tree.forceLayout()
        }
        function checkRow(row, name, path) {
            tryVerify(() => tree.itemAtCell(Qt.point(0, row)) !== null)
            const item = tree.itemAtCell(Qt.point(0, row))
            tryCompare(item.contentItem, "text", name)
            compare(item.path, path)
            compare(item.highlighted, path === "/project/zebra.afa")
            mouseClick(item, item.width / 2, item.height / 2)
            compare(openedPath, path)
        }
        function test_insert_and_remove_analysis_artifacts() {
            checkRow(1, "zebra.afa", "/project/zebra.afa")
            files.lookupPaths.splice(1, 0, "/project/species.json")
            files.insert(1, {display: "species.json", filePath: "/project/species.json"})
            tree.forceLayout()
            checkRow(1, "species.json", "/project/species.json")
            checkRow(2, "zebra.afa", "/project/zebra.afa")
            files.lookupPaths.splice(1, 1)
            files.remove(1)
            tree.forceLayout()
            checkRow(1, "zebra.afa", "/project/zebra.afa")
        }
        function test_identity_changes_at_same_row() {
            checkRow(0, "alpha.afa", "/project/alpha.afa")
            files.lookupPaths[0] = "/project/species.json"
            files.set(0, {display: "species.json", filePath: "/project/species.json"})
            checkRow(0, "species.json", "/project/species.json")
        }
        function test_sort_moves() {
            checkRow(0, "alpha.afa", "/project/alpha.afa")
            files.lookupPaths.reverse()
            files.move(0, 1, 1)
            tree.forceLayout()
            checkRow(0, "zebra.afa", "/project/zebra.afa")
            checkRow(1, "alpha.afa", "/project/alpha.afa")
        }
    }
}
