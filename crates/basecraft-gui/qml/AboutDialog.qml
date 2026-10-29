import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Wordmark and version, a randomly chosen ASCII animation, copyright, licences and citation.
AppContentDialog {
    id: dialog
    title: "About Basecraft"
    modal: true
    property string version: ""
    property color backgroundColor: palette.window
    readonly property string projectUrl: "https://github.com/hverbruggen/basecraft"
    readonly property string labUrl: "https://hverbruggen.codeberg.page/"

    // Short labels: size to the text rather than the style's minimum width.
    component FooterButton: AppButton { implicitWidth: contentItem.implicitWidth + leftPadding + rightPadding }

    component AboutLink: Label {
        id: linkLabel
        required property string targetUrl
        textFormat: Text.StyledText
        linkColor: dialog.palette.link
        activeFocusOnTab: true
        Accessible.role: Accessible.Link
        onLinkActivated: link => Qt.openUrlExternally(link)
        Keys.onReturnPressed: Qt.openUrlExternally(targetUrl)
        Keys.onSpacePressed: Qt.openUrlExternally(targetUrl)
        HoverHandler { cursorShape: linkLabel.hoveredLink ? Qt.PointingHandCursor : Qt.ArrowCursor }
        Rectangle {
            anchors.fill: parent
            anchors.margins: -2
            visible: linkLabel.activeFocus
            color: "transparent"
            border.color: dialog.palette.highlight
            radius: 2
        }
    }

    // Asks the window to open the keyboard shortcuts after this dialog closes.
    signal shortcutsRequested()
    initialFocusItem: closeButton
    // The wordmark already names the dialog; the title stays for assistive technology.
    header: Item {}

    // Themed buttons, right-aligned. They never widen the dialog: when the content
    // is narrower than one row of buttons, they split into two balanced rows.
    footer: Item {
        id: footerRow
        readonly property var widths: Array.from(actions.children, child => child.implicitWidth)
        readonly property real available: width - dialog.leftPadding - dialog.rightPadding
        // Rows the Flow would make at a given width, filling each row greedily.
        function rowsAt(width) {
            let rows = 1, used = -actions.spacing
            for (const w of widths) {
                if (used + actions.spacing + w > width + 0.5) { rows++; used = w }
                else used += actions.spacing + w
            }
            return rows
        }
        // The narrowest width that keeps the fewest rows, so a wrapped second row is as full as the first.
        readonly property real flowWidth: {
            const total = widths.reduce((a, b) => a + b, 0) + Math.max(0, widths.length - 1) * actions.spacing
            if (total <= available) return total
            const fewest = rowsAt(available)
            let best = available
            for (let w = Math.max(...widths); w < available; w += 4)
                if (rowsAt(w) === fewest) { best = w; break }
            return best
        }
        implicitWidth: Math.max(...widths) + dialog.leftPadding + dialog.rightPadding
        implicitHeight: actions.implicitHeight + dialog.topPadding + dialog.bottomPadding
        Flow {
            id: actions
            anchors.right: parent.right
            anchors.rightMargin: dialog.rightPadding
            y: dialog.topPadding
            width: footerRow.flowWidth
            spacing: 8
            Item {
                implicitWidth: learn.implicitWidth
                implicitHeight: learn.implicitHeight
                width: implicitWidth
                height: implicitHeight
                ToolTip.text: "Manual pages are not written yet"
                ToolTip.visible: learnHover.hovered
                HoverHandler { id: learnHover }
                FooterButton {
                    id: learn
                    objectName: "aboutLearn"
                    text: "Manual"
                    enabled: false
                    Accessible.description: "Manual pages are not written yet"
                }
            }
            FooterButton { objectName: "aboutShortcuts"; text: "Shortcuts"; onClicked: { dialog.close(); dialog.shortcutsRequested() } }
            FooterButton { objectName: "aboutProjectPage"; text: "GitHub"; onClicked: Qt.openUrlExternally(dialog.projectUrl) }
            FooterButton { objectName: "aboutReport"; text: "Report bug"; onClicked: Qt.openUrlExternally(dialog.projectUrl + "/issues/new") }
            FooterButton { id: closeButton; objectName: "aboutClose"; text: "Close"; onClicked: dialog.close() }
        }
    }

    body: ColumnLayout {
        spacing: 8

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: 10
            // The wordmark artwork has wide transparent margins; show only the lettering.
            Image {
                id: logo
                objectName: "aboutLogo"
                readonly property bool darkBackground: 0.2126 * dialog.backgroundColor.r
                    + 0.7152 * dialog.backgroundColor.g + 0.0722 * dialog.backgroundColor.b < 0.5
                source: Qt.resolvedUrl("../branding/wordmark-" + (darkBackground ? "dark" : "light") + ".png")
                sourceClipRect: Qt.rect(210, 336, 2636, 624)
                Layout.preferredWidth: 300
                Layout.preferredHeight: 300 * 624 / 2636
                fillMode: Image.PreserveAspectFit
                smooth: true
                mipmap: true
                Accessible.role: Accessible.Graphic
                Accessible.name: "Basecraft"
            }
            Label {
                objectName: "aboutVersion"
                text: dialog.version
                opacity: 0.7
                Layout.alignment: Qt.AlignBottom
                Accessible.name: "Version " + dialog.version
            }
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

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: 0
            AboutLink {
                objectName: "aboutCopyright"
                text: "© 2026 <a href=\"" + targetUrl + "\">Heroen Verbruggen</a>"
                targetUrl: dialog.labUrl
                Accessible.name: "Copyright 2026 Heroen Verbruggen, lab website"
            }
            Label { text: "  ·  " }
            AboutLink {
                objectName: "aboutAffiliation"
                text: "<a href=\"" + targetUrl + "\">CSIC-IEO</a>"
                targetUrl: "https://ieo.csic.es/"
                Accessible.name: "CSIC-IEO, affiliation website"
            }
            Label { text: "  ·  GPL-3.0, Apache-2.0" }
        }

        Label {
            objectName: "aboutCitation"
            Layout.alignment: Qt.AlignHCenter
            text: "Citation not yet available"
            opacity: 0.7
        }
    }
}
