// Typing into the annotation editor. Run with:
//   qmltestrunner -input crates/basecraft-gui/tests/qml -import crates/basecraft-gui
import QtQuick
import QtQuick.Controls
import QtTest
import "../../qml"

Item {
    id: root
    width: 800
    height: 600

    // Stands in for the Rust GenomeController.
    QtObject {
        id: fake
        property string accent: "red"
        property string message: ""
        property string saved: ""
        function feature_kinds() { return JSON.stringify(["gene", "CDS", "misc_feature"]) }
        function new_feature_json() {
            return JSON.stringify({ id: -1, kind: "misc_feature", location: "10..20",
                info: { valid: true, strand: "forward", partial5: false, partial3: false, length: 11, parts: 1 },
                qualifiers: [{ key: "label", value: "", flag: false }] })
        }
        function location_info(t) { return JSON.stringify({ valid: true, strand: "forward", partial5: false, partial3: false, length: 11, parts: 1 }) }
        function adjust_location(t, s, a, b) { return t }
        function save_feature(id, json) { saved = json; return "" }
        function selected_feature() { return -1 }
    }

    // Stands in for the sequence view: focused, and eats keys it sees.
    Item {
        id: view
        anchors.fill: parent
        focus: true
        property string ate: ""
        Keys.onPressed: (e) => { ate += e.text; e.accepted = true }
    }

    FeatureEditor {
        id: editor
        controller: fake
    }

    TestCase {
        name: "FeatureEditor"
        when: windowShown

        function test_typing_letters_and_digits_into_a_label() {
            view.forceActiveFocus()
            editor.openNew()
            tryCompare(editor, "opened", true)
            tryVerify(() => findChild(editor.contentItem, "qualifierValue0") !== null, 2000, "value field exists")
            const value = findChild(editor.contentItem, "qualifierValue0")
            mouseClick(value)
            for (const ch of "orf12")
                keyClick(ch)
            compare(view.ate, "", "keys leaked to the view behind the dialog")
            compare(value.text, "orf12")
            editor.save()
            verify(fake.saved.indexOf("\"value\":\"orf12\"") >= 0, fake.saved)
        }
    }
}
