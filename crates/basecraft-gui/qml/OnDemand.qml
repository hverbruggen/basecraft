import QtQuick

// A dialog built the first time it is needed instead of at startup.
Item {
    property Component sourceComponent
    property var item: null
    function dialog() { if (!item) item = sourceComponent.createObject(this); return item }
}
