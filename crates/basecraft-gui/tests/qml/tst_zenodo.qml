import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: root
    width: 1000; height: 900
    Component { id: factory; ZenodoDialog {} }
    QtObject {
        id: backend
        property bool busy: false
        property string message: ""
        property string outcome: ""
        property var requests: []
        property string runtimeToken: ""
        property int cancellations: 0
        function submit(project, request) { requests.push(JSON.parse(request)); busy = true; return true }
        function submit_zenodo(project, request, token) { runtimeToken = token; return submit(project, request) }
        function poll() { }
        function cancel() { cancellations++ }
    }
    TestCase {
        name: "ZenodoPreparation"
        when: windowShown
        function init() { backend.busy = false; backend.requests = []; backend.runtimeToken = ""; backend.cancellations = 0; failOnWarning(/Binding loop|TypeError|ReferenceError/) }
        function dialog() { return createTemporaryObject(factory, root, {backend: backend}) }
        function field(d, name) { return findChild(d, name) }
        function prepared(d) {
            d.show(["/project/source.fa"], "/project", {})
            backend.busy = false
            backend.outcome = JSON.stringify({state: "completed", data: {selection: "abc", files: [{name: "source.fa", snapshot: "/project/run/snapshot", size: 4, sha256: "hash"}], total_bytes: 4}, artifacts: [{path: "/project/run/selection"}]})
            d.completed()
        }
        function settings(d) {
            field(d, "zenodoTitle").text = "Synthetic dataset"
            field(d, "zenodoDescription").text = "Synthetic fixture"
            field(d, "zenodoCreators").text = "Example, A."
            field(d, "zenodoToken").text = "synthetic-secret"
        }
        function test_explicit_preview_then_secret_separate_from_request() {
            const d = dialog(); prepared(d); settings(d)
            compare(backend.requests[0].operation, "basecraft.zenodo/prepare@1")
            verify(field(d, "zenodoFiles").text.includes("source.fa — 4 bytes"))
            verify(field(d, "zenodoFiles").text.includes("hash"))
            d.start()
            compare(backend.requests[1].operation, "basecraft.zenodo/draft@1")
            compare(backend.requests[1].inputs.files[0].path, "/project/run/snapshot")
            compare(backend.runtimeToken, "synthetic-secret")
            compare(field(d, "zenodoToken").text, "")
            verify(!JSON.stringify(backend.requests).includes("synthetic-secret"))
            d.start(); compare(backend.requests.length, 2)
            backend.busy = false; d.close()
        }
        function test_required_fields_block_upload() {
            const d = dialog(); prepared(d); d.start()
            compare(backend.requests.length, 1); verify(d.error.length > 0); d.close()
        }
        function test_known_draft_retry_uses_receipt_and_same_metadata() {
            const d = dialog(); prepared(d); settings(d); d.start()
            backend.busy = false
            backend.outcome = JSON.stringify({state: "completed", data: {status: "upload_unknown", draft_id: 123, url: "https://sandbox.zenodo.org/uploads/123", message: "Partial upload"}, artifacts: [{path: "/project/run/receipt"}]})
            d.completed(); verify(d.canRetry)
            verify(!field(d, "zenodoTitle").enabled)
            verify(!field(d, "zenodoLicense").enabled)
            // Even a programmatic edit cannot change an existing draft retry.
            field(d, "zenodoTitle").text = "Do not submit this change"
            field(d, "zenodoToken").text = "another-secret"
            d.start()
            compare(backend.requests[2].inputs.resume[0].path, "/project/run/receipt")
            compare(backend.requests[2].parameters.title, "Synthetic dataset")
            backend.busy = false; d.close()
        }
        function test_rejected_authentication_can_retry_creation() {
            const d = dialog(); prepared(d); settings(d); d.start()
            backend.busy = false
            backend.outcome = JSON.stringify({state: "completed", data: {status: "not_sent", draft_id: null, message: "Authentication expired"}, artifacts: [{path: "/project/run/receipt"}]})
            d.completed(); verify(d.canRetry)
            field(d, "zenodoToken").text = "new-secret"
            d.start(); compare(backend.requests.length, 3)
            compare(backend.requests[2].inputs.resume[0].path, "/project/run/receipt")
            backend.busy = false; d.close()
        }
        function test_rejected_metadata_can_be_corrected_in_place() {
            const d = dialog(); prepared(d); settings(d); d.start()
            backend.busy = false
            backend.outcome = JSON.stringify({state: "completed", data: {status: "not_sent", draft_id: null, message: "Metadata rejected"}, artifacts: [{path: "/project/run/receipt"}]})
            d.completed(); verify(d.canRetry)
            for (const name of ["zenodoTitle", "zenodoDescription", "zenodoCreators", "zenodoLicense", "zenodoAccess"]) verify(field(d, name).enabled)
            verify(!field(d, "zenodoService").enabled)
            field(d, "zenodoTitle").text = "Corrected title"
            field(d, "zenodoLicense").text = "cc-zero"
            field(d, "zenodoToken").text = "new-secret"
            d.start()
            compare(backend.requests.length, 3)
            compare(backend.requests[2].parameters.title, "Corrected title")
            compare(backend.requests[2].parameters.license, "cc-zero")
            compare(backend.requests[2].inputs.selection[0].path, "/project/run/selection")
            compare(backend.requests[2].inputs.files[0].path, "/project/run/snapshot")
            compare(backend.requests[2].inputs.resume[0].path, "/project/run/receipt")
            verify(!field(d, "zenodoTitle").enabled, "Freeze metadata during the corrected request")
            d.start(); compare(backend.requests.length, 3)
            backend.busy = false; d.close()
        }
        function test_unknown_creation_never_blindly_retries() {
            const d = dialog(); prepared(d); settings(d); d.start()
            backend.busy = false
            backend.outcome = JSON.stringify({state: "completed", data: {status: "creation_unknown", draft_id: null, message: "Inspect Zenodo"}, artifacts: [{path: "/project/run/receipt"}]})
            d.completed(); verify(!d.canRetry)
            verify(!field(d, "zenodoTitle").enabled)
            verify(!field(d, "zenodoLicense").enabled)
            d.start(); compare(backend.requests.length, 2); d.close()
        }
        function test_keyboard_field_access_escape_cancels_and_clears_token() {
            const d = dialog(); prepared(d); settings(d)
            tryCompare(d, "opened", true)
            tryVerify(() => field(d, "zenodoTitle").activeFocus)
            keyClick(Qt.Key_D, Qt.AltModifier)
            tryVerify(() => field(d, "zenodoDescription").activeFocus)
            backend.busy = true
            keyClick(Qt.Key_Escape)
            tryCompare(d, "visible", false)
            compare(backend.cancellations, 1); compare(field(d, "zenodoToken").text, "")
            backend.busy = false
        }
    }
}
