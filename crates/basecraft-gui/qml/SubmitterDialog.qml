import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// The submitter's contact details and affiliation, entered once and used
// for every submission's template (.sbt). Stored as a preference, not in
// the project. Authors and the title belong to each submission.
AppFormDialog {
    id: dialog
    objectName: "submitterDialog"
    title: "Submitter"
    standardButtons: Dialog.Save | Dialog.Cancel
    initialFocusItem: firstName
    // Preferences.submitter, as stored.
    property var submitter: ({})
    signal saved(var submitter)
    readonly property var fieldNames: ["first_name", "middle_initials", "last_name", "email", "phone",
        "institution", "department", "street", "city", "state", "postal_code", "country"]
    readonly property bool complete: firstName.text.trim().length && lastName.text.trim().length
        && email.text.trim().indexOf("@") > 0 && institution.text.trim().length
        && city.text.trim().length && country.text.trim().length
    function show(current) {
        submitter = current || {}
        for (const name of fieldNames) {
            const field = fieldFor(name)
            field.text = submitter[name] || ""
        }
        open()
    }
    function fieldFor(name) {
        return { first_name: firstName, middle_initials: middle, last_name: lastName, email: email,
            phone: phone, institution: institution, department: department, street: street,
            city: city, state: state, postal_code: postalCode, country: country }[name]
    }
    function values() {
        const out = {}
        for (const name of fieldNames) out[name] = fieldFor(name).text.trim()
        return out
    }
    onOpened: standardButton(Dialog.Save).enabled = Qt.binding(() => dialog.complete)
    onAccepted: saved(values())

    Label {
        text: "Written to the submission template of every GenBank package. First and last name, email, institution, city and country are required."
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    TextField { id: firstName; objectName: "submitterFirstName"; Kirigami.FormData.label: "First name:" }
    TextField { id: middle; Kirigami.FormData.label: "Middle initials:"; placeholderText: "M." }
    TextField { id: lastName; objectName: "submitterLastName"; Kirigami.FormData.label: "Last name:" }
    TextField { id: email; objectName: "submitterEmail"; Kirigami.FormData.label: "Email:" }
    TextField { id: phone; Kirigami.FormData.label: "Phone:" }
    TextField { id: institution; objectName: "submitterInstitution"; Kirigami.FormData.label: "Institution:" }
    TextField { id: department; Kirigami.FormData.label: "Department:" }
    TextField { id: street; Kirigami.FormData.label: "Street:" }
    TextField { id: city; objectName: "submitterCity"; Kirigami.FormData.label: "City:" }
    TextField { id: state; Kirigami.FormData.label: "State or province:" }
    TextField { id: postalCode; Kirigami.FormData.label: "Postal code:" }
    TextField { id: country; objectName: "submitterCountry"; Kirigami.FormData.label: "Country:" }
}
