import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppDialog {
    id: dialog
    title: "Document text representation"
    property var source: null
    property int page: 0
    property var snapshot: ({page: 0, pages: 1, text: ""})
    initialFocusItem: textView
    standardButtons: Dialog.Close
    implicitWidth: 800
    function loadPage(requested) {
        if (!source) return
        snapshot = JSON.parse(source.accessible_page(requested))
        page = snapshot.page
        textView.cursorPosition = 0
        textView.forceActiveFocus(Qt.OtherFocusReason)
    }
    onOpened: loadPage(0)
    onClosed: source = null
    contentItem: ColumnLayout {
        Label {
            Layout.fillWidth: true
            text: "Read-only snapshot of the current document. Coordinates are one-based."
            wrapMode: Text.Wrap
        }
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredHeight: 400
            Layout.minimumHeight: 40
            contentWidth: availableWidth
            clip: true
            TextArea {
                id: textView
                text: dialog.snapshot.text
                textFormat: TextEdit.PlainText
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.Wrap
                Accessible.name: "Document text, page " + (dialog.page + 1)
            }
        }
        RowLayout {
            Button { text: "Previous"; enabled: dialog.page > 0; onClicked: dialog.loadPage(dialog.page - 1) }
            Label { text: "Page " + (dialog.page + 1) + " of " + dialog.snapshot.pages }
            Button { text: "Next"; enabled: dialog.page + 1 < dialog.snapshot.pages; onClicked: dialog.loadPage(dialog.page + 1) }
            Button { text: "Refresh"; onClicked: dialog.loadPage(dialog.page) }
        }
    }
}
