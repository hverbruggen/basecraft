import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

RowLayout {
    id: root
    required property var controller
    spacing: 8
    readonly property var settings: controller ? JSON.parse(controller.review_settings) : ({strong_score:30, paired_score:20, ambiguity_percent:35, allow_weak_disagreement:true, weak_support_percent:80, check_reference:true, reference_percent:0, reference_min_rows:1})
    function apply(key, value) {
        if (!controller) return
        const next = Object.assign({}, settings)
        next[key] = value
        controller.apply_review_settings(JSON.stringify(next))
    }
    component Threshold: ColumnLayout {
        id: field
        property string label
        property string tip
        property int inputWidth: 64
        property alias value: spin.value
        property alias from: spin.from
        property alias to: spin.to
        signal edited()
        spacing: 0
        Label { id: thresholdLabel; text: field.label }
        SpinBox {
            id: spin
            Accessible.labelledBy: thresholdLabel
            Accessible.name: thresholdLabel.text
            Accessible.description: field.tip
            objectName: field.objectName + "Input"
            implicitWidth: Math.max(field.inputWidth, implicitContentWidth + leftPadding + rightPadding)
            editable: true
            live: true
            leftPadding: 18
            rightPadding: 18
            up.indicator: AppIcon {
                x: spin.width - width; width: 16; height: spin.height
                name: "add"; size: 12; color: spin.palette.buttonText
                opacity: spin.value >= spin.to ? 0.35 : 1
            }
            down.indicator: AppIcon {
                width: 16; height: spin.height
                name: "minus"; size: 12; color: spin.palette.buttonText
                opacity: spin.value <= spin.from ? 0.35 : 1
            }
            onValueModified: field.edited()
            ToolTip.text: field.tip
            ToolTip.visible: hovered
            ToolTip.delay: 400
        }
    }
    Threshold {
        objectName: "strongScore"
        label: "One clean read ≥"
        tip: "Minimum Tracy score for one clean supporting read; cannot be below the two-read score. Changing this re-evaluates automatic calls."
        from: root.settings.paired_score; to: 100; value: root.settings.strong_score
        onEdited: root.apply("strong_score", value)
    }
    Threshold {
        label: "Two clean reads ≥"
        tip: "Minimum Tracy score for each of two clean agreeing reads."
        from: 0; to: root.settings.strong_score; value: root.settings.paired_score
        onEdited: root.apply("paired_score", value)
    }
    ToolSeparator { Layout.fillHeight: true }
    Threshold {
        label: "Ambiguous ≥ %"
        tip: "Secondary/primary signal at the same Tracy call position: at or above this percentage the read is ambiguous."
        from: 1; to: 100; value: root.settings.ambiguity_percent
        onEdited: root.apply("ambiguity_percent", value)
    }
    CheckBox { text: "Allow weak\ndisagreements"; checked: root.settings.allow_weak_disagreement; onClicked: root.apply("allow_weak_disagreement", checked) }
    Threshold {
        label: "Candidate peak ≥ %"
        tip: "In a weak read, the candidate channel must reach this percentage of the strongest channel at that call position."
        enabled: root.settings.allow_weak_disagreement
        from: 1; to: 100; value: root.settings.weak_support_percent
        onEdited: root.apply("weak_support_percent", value)
    }
    ToolSeparator { Layout.fillHeight: true }
    CheckBox { text: "Check\nreference"; checked: root.settings.check_reference; onClicked: root.apply("check_reference", checked) }
    Threshold {
        label: "Reference ≥ %"
        tip: "Minimum candidate frequency among all A/C/G/T reference rows. At 0%, any observed occurrence qualifies."
        enabled: root.settings.check_reference
        from: 0; to: 100; value: root.settings.reference_percent
        onEdited: root.apply("reference_percent", value)
    }
    Threshold {
        inputWidth: 88
        label: "Reference rows ≥"
        tip: "Minimum number of informative A/C/G/T observations in the mapped alignment column."
        enabled: root.settings.check_reference
        from: 1; to: 1000000; value: root.settings.reference_min_rows
        onEdited: root.apply("reference_min_rows", value)
    }
    Label { text: "Hover for details"; opacity: 0.55 }
    Item { Layout.fillWidth: true }
}
