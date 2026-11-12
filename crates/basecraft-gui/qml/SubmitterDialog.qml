import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Contact details, entered once: the GenBank submitter (written to every
// submission's template, .sbt) and the email other services such as NCBI
// BLAST receive, which defaults to the submitter's. Stored as a preference,
// not in the project. Authors and the title belong to each submission.
AppFormDialog {
    id: dialog
    objectName: "submitterDialog"
    title: "Submitter"
    standardButtons: Dialog.Save | Dialog.Cancel
    initialFocusItem: firstName
    // Preferences.submitter, as stored.
    property var submitter: ({})
    // Service emails that differ from the submitter's, by service.
    signal saved(var submitter, var services)
    readonly property var fieldNames: ["first_name", "middle_initials", "last_name", "email", "phone",
        "institution", "department", "street", "city", "state", "postal_code", "country"]
    readonly property bool complete: firstName.text.trim().length && lastName.text.trim().length
        && email.text.trim().indexOf("@") > 0 && institution.text.trim().length
        && city.text.trim().length && country.text.trim().length
    function show(current, services) {
        submitter = current || {}
        blastEmail.text = (services || {}).blast || ""
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
    onAccepted: saved(values(), {blast: blastEmail.text.trim()})

    Label { Kirigami.FormData.isSection: true; text: "GenBank submission"; font.bold: true }
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
    Label { Kirigami.FormData.isSection: true; text: "Other services"; font.bold: true }
    TextField {
        id: blastEmail
        objectName: "blastEmail"
        Kirigami.FormData.label: "NCBI BLAST email:"
        placeholderText: email.text.trim().length ? email.text.trim() : "The submission email"
    }
    Label {
        text: "Left empty, BLAST uses the submission email above."
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
