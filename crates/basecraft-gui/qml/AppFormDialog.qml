import QtQuick
import QtQuick.Controls.Basic
import org.kde.kirigami as Kirigami

AppDialog {
    id: dialog
    default property alias fields: form.data
    // FormLayout adapts to available width; it cannot also determine that width.
    implicitWidth: 680
    contentItem: FocusScrollView {
        id: scroll
        clip: true
        implicitHeight: Math.min(form.implicitHeight, Math.max(80,
            (Overlay.overlay ? Overlay.overlay.height : 760) - 160))
        contentWidth: availableWidth
        Kirigami.FormLayout {
            id: form
            width: scroll.availableWidth
        }
    }
}
