import QtQuick

// A dialog built the first time it is needed instead of at startup.
Loader {
    active: false
    function dialog() { active = true; return item }
}
