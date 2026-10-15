import QtQuick
import QtQuick.Layouts

// Related fields stay together, stacking when their natural widths do not fit.
GridLayout {
    id: row
    readonly property real preferredRowWidth: {
        let total = 0
        let count = 0
        for (const field of children) {
            if (!field.visible) continue
            total += Math.ceil(field.implicitWidth)
            count++
        }
        return total + Math.max(0, count - 1) * columnSpacing
    }
    readonly property int fieldCount: {
        let count = 0
        for (const field of children) if (field.visible) count++
        return count
    }
    implicitWidth: preferredRowWidth
    columns: width >= preferredRowWidth ? Math.max(1, fieldCount) : 1
    columnSpacing: 12
    rowSpacing: 8
    Layout.fillWidth: true
    Layout.preferredWidth: preferredRowWidth
}
