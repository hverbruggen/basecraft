import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Wordmark, a randomly chosen ASCII animation, credits, licences and citation.
AppContentDialog {
    id: dialog
    title: "About Basecraft"
    modal: true
    standardButtons: Dialog.Close
    property string version: ""
    property color backgroundColor: palette.window
    readonly property string projectUrl: "https://github.com/hverbruggen/basecraft"

    body: ColumnLayout {
        spacing: 12

        BrandLogo {
            id: logo
            objectName: "aboutLogo"
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: Math.min(360, dialog.availableWidth)
            Layout.preferredHeight: width * 420 / 1020
            backgroundColor: dialog.backgroundColor
        }

        Label {
            objectName: "aboutTagline"
            Layout.fillWidth: true
            text: logo.tagline
            color: logo.taglineColor
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
        }

        // Loaded on opening, so the frame data never slows down startup.
        Loader {
            id: art
            objectName: "aboutArt"
            Layout.alignment: Qt.AlignHCenter
            active: dialog.visible
            asynchronous: false
            Component.onCompleted: setSource("AsciiArtPlayer.qml", { font: dialog.font })
        }

        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 16
            rowSpacing: 6

            Label { text: "Version"; opacity: 0.7; Layout.alignment: Qt.AlignTop }
            Label { objectName: "aboutVersion"; text: dialog.version; Layout.fillWidth: true; wrapMode: Text.Wrap }

            Label { text: "Created by"; opacity: 0.7; Layout.alignment: Qt.AlignTop }
            Label { text: "Heroen Verbruggen"; Layout.fillWidth: true; wrapMode: Text.Wrap }

            Label { text: "Copyright"; opacity: 0.7; Layout.alignment: Qt.AlignTop }
            Label { text: "© 2026 Heroen Verbruggen"; Layout.fillWidth: true; wrapMode: Text.Wrap }

            Label { text: "Licence"; opacity: 0.7; Layout.alignment: Qt.AlignTop }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: "GNU GPL 3.0 or later. The plugin protocol and Python SDK are Apache 2.0. "
                    + "The wordmark lettering is derived from JetBrains Mono (SIL Open Font License 1.1)."
            }

            Label { text: "Citation"; opacity: 0.7; Layout.alignment: Qt.AlignTop }
            Label {
                objectName: "aboutCitation"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: "No publication yet. Until there is one, please cite the source code repository and the version you used."
            }

            Label { text: "Source code"; opacity: 0.7; Layout.alignment: Qt.AlignTop }
            Label { objectName: "aboutUrl"; text: dialog.projectUrl; Layout.fillWidth: true; wrapMode: Text.WrapAnywhere }
        }

        AppButton {
            objectName: "aboutProjectPage"
            text: "Open the project page"
            Accessible.description: "Opens " + dialog.projectUrl + " in the web browser"
            onClicked: Qt.openUrlExternally(dialog.projectUrl)
        }
    }
}
