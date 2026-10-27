import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

RowLayout {
    id: page
    RibbonMetrics { id: metrics; control: page }
    // Match the explicit spacing used by the Delimitation ribbon page.
    spacing: 6 * metrics.factor
}
