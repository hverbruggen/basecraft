import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Wordmark, a randomly chosen ASCII animation, version, copyright, licences and citation.
AppContentDialog {
    id: dialog
    title: "About Basecraft"
    modal: true
    property string version: ""
    property color backgroundColor: palette.window
    readonly property string projectUrl: "https://github.com/hverbruggen/basecraft"
    readonly property string labUrl: "https://hverbruggen.codeberg.page/"

    // Asks the window to open the keyboard shortcuts after this dialog closes.
    signal shortcutsRequested()
    initialFocusItem: closeButton

    // One row of themed buttons, right-aligned, wrapping in narrow windows.
    footer: Item {
        readonly property real naturalWidth: {
            let total = Math.max(0, actions.children.length - 1) * actions.spacing
            for (const child of actions.children) total += child.implicitWidth
            return total
        }
        implicitWidth: naturalWidth + dialog.leftPadding + dialog.rightPadding
        implicitHeight: actions.implicitHeight + dialog.topPadding + dialog.bottomPadding
        Flow {
            id: actions
            anchors.right: parent.right
            anchors.rightMargin: dialog.rightPadding
            y: dialog.topPadding
            width: Math.min(parent.width - dialog.leftPadding - dialog.rightPadding, parent.naturalWidth)
            spacing: 8
            Item {
                implicitWidth: learn.implicitWidth
                implicitHeight: learn.implicitHeight
                width: implicitWidth
                height: implicitHeight
                ToolTip.text: "Manual pages are not written yet"
                ToolTip.visible: learnHover.hovered
                HoverHandler { id: learnHover }
                AppButton {
                    id: learn
                    objectName: "aboutLearn"
                    text: "Learn"
                    enabled: false
                    Accessible.description: "Manual pages are not written yet"
                }
            }
            AppButton { objectName: "aboutShortcuts"; text: "Keyboard shortcuts"; onClicked: { dialog.close(); dialog.shortcutsRequested() } }
            AppButton { objectName: "aboutProjectPage"; text: "Project page"; onClicked: Qt.openUrlExternally(dialog.projectUrl) }
            AppButton { objectName: "aboutReport"; text: "Report a problem"; onClicked: Qt.openUrlExternally(dialog.projectUrl + "/issues/new") }
            AppButton { id: closeButton; objectName: "aboutClose"; text: "Close"; onClicked: dialog.close() }
        }
    }

    body: ColumnLayout {
        spacing: 10

        BrandLogo {
            id: logo
            objectName: "aboutLogo"
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: Math.min(260, dialog.availableWidth)
            Layout.preferredHeight: width * 420 / 1020
            backgroundColor: dialog.backgroundColor
        }

        // Loaded on opening, so the frame data never slows down startup.
        Loader {
            id: art
            objectName: "aboutArt"
            Layout.alignment: Qt.AlignHCenter
            active: dialog.visible
            Component.onCompleted: setSource("AsciiArtPlayer.qml", { font: dialog.font })
            Binding { target: art.item; property: "dark"; value: logo.darkBackground; when: art.item !== null }
        }

        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 16
            rowSpacing: 4

            Label { text: "Version"; opacity: 0.7 }
            Label { objectName: "aboutVersion"; text: dialog.version; Layout.fillWidth: true; wrapMode: Text.Wrap }

            Label { text: "Copyright"; opacity: 0.7 }
            Label {
                id: copyright
                objectName: "aboutCopyright"
                textFormat: Text.StyledText
                linkColor: dialog.palette.link
                text: "© 2026 <a href=\"" + dialog.labUrl + "\">Heroen Verbruggen</a>"
                activeFocusOnTab: true
                Accessible.role: Accessible.Link
                Accessible.name: "Heroen Verbruggen, lab website"
                onLinkActivated: link => Qt.openUrlExternally(link)
                Keys.onReturnPressed: Qt.openUrlExternally(dialog.labUrl)
                Keys.onSpacePressed: Qt.openUrlExternally(dialog.labUrl)
                HoverHandler { cursorShape: copyright.hoveredLink ? Qt.PointingHandCursor : Qt.ArrowCursor }
                Rectangle {
                    anchors.fill: parent
                    anchors.margins: -2
                    visible: copyright.activeFocus
                    color: "transparent"
                    border.color: dialog.palette.highlight
                    radius: 2
                }
            }

            Label { text: "Licence"; opacity: 0.7 }
            Label { text: "GPL-3.0-or-later, Apache-2.0"; Layout.fillWidth: true; wrapMode: Text.Wrap }

            Label { text: "Citation"; opacity: 0.7 }
            Label { objectName: "aboutCitation"; text: "Not yet available"; Layout.fillWidth: true; wrapMode: Text.Wrap }
        }
    }
}
