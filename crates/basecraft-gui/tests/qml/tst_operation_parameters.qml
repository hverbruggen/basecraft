import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 600; height: 700
    Component { id: formComponent; OperationParameters { width: 500 } }
    Component { id: scalarComponent; OperationParameterValue { width: 400 } }
    TestCase {
        name: "OperationParameters"
        when: windowShown
        function test_numbers_preserve_fraction_and_reject_invalid() {
            const input = createTemporaryObject(scalarComponent, parent, {schema: {type: "number", exclusiveMinimum: 0}, fieldName: "age"})
            const text = findChild(input, "text_age")
            text.text = "0.125"
            compare(input.value(), 0.125)
            text.text = "2e-3"
            compare(input.value(), 0.002)
            for (const invalid of ["", "NaN", "Infinity", "0", "-1", "0x10"]) {
                text.text = invalid
                let rejected = false
                try { input.value() } catch (error) { rejected = true }
                verify(rejected, invalid)
            }
        }
        function test_exclusive_choice_and_nested_constraints() {
            failOnWarning(/Binding loop|ReferenceError|TypeError/)
            const form = createTemporaryObject(formComponent, parent, {schema: {
                type: "object", properties: {
                    lambda: {type: "number", exclusiveMinimum: 0},
                    cv: {type: "object", properties: {count: {type: "integer", default: 3}}, required: ["count"]},
                    constraints: {type: "array", maxItems: 2, items: {type: "object", properties: {age: {type: "number"}}, required: ["age"]}}
                }, oneOf: [{required: ["lambda"]}, {required: ["cv"]}]
            }})
            tryVerify(() => findChild(form, "text_lambda") !== null)
            findChild(form, "text_lambda").text = "0.5"
            compare(JSON.stringify(form.value()), '{"lambda":0.5}')
            findChild(form, "parameterAlternative").currentIndex = 1
            tryVerify(() => findChild(form, "text_count") !== null)
            compare(JSON.stringify(form.value()), '{"cv":{"count":3}}')
            findChild(form, "include_constraints").checked = true
            tryVerify(() => findChild(form, "add_constraints") !== null)
            findChild(form, "add_constraints").clicked()
            tryVerify(() => findChild(form, "text_age") !== null)
            findChild(form, "text_age").text = "1.25"
            compare(form.value(), {cv: {count: 3}, constraints: [{age: 1.25}]})
        }
        function test_defaults_and_required_values() {
            const form = createTemporaryObject(formComponent, parent, {schema: {type: "object", properties: {
                mode: {type: "string", enum: ["first", "second"], default: "second"},
                count: {type: "integer", minimum: 1}
            }, required: ["mode", "count"]}})
            tryVerify(() => findChild(form, "text_count") !== null)
            findChild(form, "text_count").text = "7"
            compare(form.value(), {mode: "second", count: 7})
        }
    }
}
