import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

RowLayout {
    id: page
    RibbonMetrics { id: metrics; control: page }
    spacing: Kirigami.Units.smallSpacing * 2 * metrics.factor
}
