import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 900; height: 700
    Component { id: colorFactory; SpeciesColorDialog {} }
    Component { id: combineFactory; CombineHypothesesDialog {} }
    Component { id: wheelFactory; ColorWheel {} }
    SignalSpy { id: applied; signalName: "colorsChosen" }
    SignalSpy { id: chosen; signalName: "chosenHypotheses" }
    readonly property var groups: [
        {id: "g1", label: "Species 1", members: 3, color: "#3d5a8a", theme: "#3d5a8a"},
        {id: "g2", label: "Species 2", members: 2, color: "#123456", theme: "#b03a2e"}
    ]
    readonly property var files: [{
        path: "/a.spart", file: "a.spart", specimens: 8, hypotheses: [
            {id: "best", name: "ASAP species hypothesis 1 (3 species, score 1.5)", code: "A", method: "ASAP", species: 3, unassigned: 0, best: true, parent: null, scores: [{value: 1.5, scale: "ASAP rank score"}]},
            {id: "second", name: "ASAP species hypothesis 2 (5 species, score 3.5)", code: "A", method: "ASAP", species: 5, unassigned: 0, best: false, parent: null, scores: [{value: 3.5, scale: "ASAP rank score"}]},
            {id: "copy", name: "Edited", code: "E", method: "Edited copy", species: 3, unassigned: 1, best: false, parent: "second", scores: []}
        ]}, {
        path: "/b.spart", file: "b.spart", specimens: 8, hypotheses: [
            {id: "manual", name: "Manual", code: "M", method: "Manual", species: 2, unassigned: 0, best: false, parent: null, scores: []}
        ]}]

    TestCase {
        name: "SpeciesDialogs"
        when: windowShown

        function test_wheel_keeps_hue_and_reports_edits() {
            const wheel = createTemporaryObject(wheelFactory, parent, {color: "#ff0000"})
            verify(wheel)
            wheel.pick(1 / 3, 1, 0.5)
            verify(Qt.colorEqual(wheel.color, Qt.hsva(1 / 3, 1, 0.5, 1)))
            // Black keeps the hue so brightening returns to the same colour.
            wheel.pick(1 / 3, 1, 0)
            wheel.pick(wheel.hue, wheel.saturation, 0.5)
            verify(Qt.colorEqual(wheel.color, Qt.hsva(1 / 3, 1, 0.5, 1)))
        }

        function test_colors_apply_hex_and_theme_reset() {
            const dialog = createTemporaryObject(colorFactory, parent)
            applied.target = dialog; applied.clear()
            dialog.show("ASAP 1", groups)
            tryCompare(dialog, "opened", true)
            // The dialog focuses its species list once it has opened.
            tryCompare(findChild(dialog, "speciesColorList"), "activeFocus", true)
            const hex = findChild(dialog, "colorHex")
            compare(hex.text, "#3d5a8a")
            mouseClick(hex)
            tryCompare(hex, "activeFocus", true)
            hex.selectAll()
            for (const c of "#00ff00") keyClick(c)
            compare(dialog.colorOf(0), "#00ff00")
            dialog.current = 1
            compare(hex.text, "#123456")
            dialog.edits = Object.assign({}, dialog.edits, {g2: ""})
            compare(dialog.colorOf(1), "#b03a2e", "An empty choice shows the theme colour")
            dialog.accept()
            compare(applied.count, 1)
            compare(applied.signalArguments[0][0].g1, "#00ff00")
            compare(applied.signalArguments[0][0].g2, "")
        }

        function test_combine_starts_with_best_and_keeps_sources() {
            const dialog = createTemporaryObject(combineFactory, parent)
            chosen.target = dialog; chosen.clear()
            dialog.show(files)
            tryCompare(dialog, "opened", true)
            verify(dialog.isChosen("/a.spart", "best"))
            verify(dialog.isChosen("/b.spart", "manual"))
            // A ranked alternative starts unchecked unless a checked copy needs it.
            verify(dialog.isChosen("/a.spart", "copy"))
            verify(dialog.isChosen("/a.spart", "second"))
            dialog.toggle(files[0], files[0].hypotheses[1], false)
            verify(!dialog.isChosen("/a.spart", "copy"), "Dropping a source drops its copies")
            dialog.accept()
            compare(chosen.count, 1)
            const selection = JSON.parse(chosen.signalArguments[0][0])
            compare(selection["/a.spart"], ["best"])
            compare(selection["/b.spart"], ["manual"])
        }

        function test_combine_boxes_sit_left_of_their_text() {
            const dialog = createTemporaryObject(combineFactory, parent)
            dialog.show(files)
            tryCompare(dialog, "opened", true)
            const box = findChild(dialog.body, "combine-second")
            const text = box.parent.children[1]
            // Checked because the edited copy of it is.
            verify(box.checked)
            // Wait for the rows to be laid out before clicking them.
            tryVerify(() => dialog.contentItem.height > box.height * 3)
            verify(box.x + box.width <= text.x, "The checkbox does not overlap its text")
            verify(box.width < 30)
            mouseClick(text)
            verify(!dialog.isChosen("/a.spart", "second"), "Clicking the text toggles the box")
            tryCompare(box, "checked", false)
            verify(!findChild(dialog.body, "combine-copy").checked, "Its copy follows")
            mouseClick(findChild(dialog.body, "combine-copy"))
            tryCompare(box, "checked", true)
        }

        function test_combine_needs_one_choice() {
            const dialog = createTemporaryObject(combineFactory, parent)
            dialog.show(files)
            tryCompare(dialog, "opened", true)
            for (const file of files) for (const h of file.hypotheses) dialog.toggle(file, h, false)
            compare(dialog.count, 0)
            verify(!dialog.standardButton(Dialog.Ok).enabled)
        }
    }
}
