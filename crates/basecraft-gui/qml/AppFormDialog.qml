import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Templates as Templates
import org.kde.kirigami as Kirigami

AppDialog {
    id: dialog
    default property alias fields: form.data
    FontMetrics { id: metrics; font: dialog.font }
    readonly property real naturalFormWidth: {
        let fields = 0
        let captions = 0
        let sections = 0
        for (const field of form.children) {
            if (!field.Accessible.labelledBy) continue
            const hint = field.Layout.preferredWidth > 0 ? field.Layout.preferredWidth : field.implicitWidth
            if (field.Kirigami.FormData.isSection) sections = Math.max(sections, hint)
            else {
                fields = Math.max(fields, hint)
                captions = Math.max(captions, metrics.advanceWidth(field.Kirigami.FormData.label))
            }
        }
        // FormLayout's trailing placeholder includes section widths as well as
        // field widths, even though sections span both columns.
        return Math.ceil(Math.max(sections, fields) + captions + 12)
    }
    readonly property real preferredFormWidth: Math.min(naturalFormWidth, maximumContentWidth)
    // Keep small forms compact; allow wider forms up to the shared window-relative limit.
    implicitWidth: Math.max(implicitHeaderWidth, implicitFooterWidth,
        preferredFormWidth + leftPadding + rightPadding)
    function inheritLabelFonts() {
        // FormLayout exposes its generated captions through this relationship.
        // Kirigami headings otherwise use the platform font, ignoring window.font.
        for (const field of form.children) {
            const label = field.Accessible.labelledBy
            if (!label) continue
            // Let text-bearing controls shrink and wrap/elide in narrow forms.
            if (field instanceof Templates.ComboBox || field instanceof Templates.TextField
                || field instanceof Templates.CheckBox) field.Layout.fillWidth = true
            label.font = Qt.binding(() => dialog.font)
            label.color = Qt.binding(() => dialog.palette.windowText)
            if (!field.Accessible.name.length && field.Kirigami.FormData.label.length)
                field.Accessible.name = Qt.binding(() => field.Kirigami.FormData.label)
        }
    }
    contentItem: FocusScrollView {
        id: scroll
        clip: true
        implicitHeight: Math.min(form.implicitHeight, Math.max(80,
            (Overlay.overlay ? Overlay.overlay.height : 760) - 160))
        contentWidth: availableWidth
        Kirigami.FormLayout {
            id: form
            width: scroll.availableWidth
            implicitWidth: dialog.preferredFormWidth
            wideMode: width >= dialog.naturalFormWidth
            onChildrenChanged: Qt.callLater(dialog.inheritLabelFonts)
            Component.onCompleted: Qt.callLater(dialog.inheritLabelFonts)
        }
    }
}
