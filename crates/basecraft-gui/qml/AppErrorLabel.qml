import QtQuick
import QtQuick.Controls.Basic

Label {
    Accessible.role: Accessible.AlertMessage
    Accessible.name: text
    function announceMessage() {
        if (visible && text.length && Accessible.role === Accessible.AlertMessage) Accessible.announce(text)
    }
    // Wait for the message's visibility binding; coalesce changes in one turn.
    onTextChanged: Qt.callLater(announceMessage)
    onVisibleChanged: if (visible) Qt.callLater(announceMessage)
}
