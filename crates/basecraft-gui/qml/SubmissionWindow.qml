import QtQuick

// The GenBank submission editor, bound to the project's submission and the
// job that validates it with table2asn.
SubmissionDialog {
    id: submissionDialog
    required property var app
    required property var submission
    required property var validationJobs
    required property var specimenData
    required property var accessionTableDialog
    onContactDetailsRequested: app.showContacts()
    document: JSON.parse(submission.document || "null")
    review: JSON.parse(submission.review || '{"rows":[],"issues":[]}')
    pkg: JSON.parse(submission.package || "null")
    geneticCodes: JSON.parse(submission.genetic_codes())
    path: submission.path
    message: validationJobs.busy ? validationJobs.message : submission.message
    messageKind: submission.message_kind
    canUndo: submission.can_undo
    canRedo: submission.can_redo
    validating: validationJobs.busy
    reviewStage: submission.review_stage
    validationStage: validationJobs.message
    reviewing: submission.reviewing
    parseAuthors: text => JSON.parse(submission.preview_authors(text))
    onDetailsRequested: details => submission.set_details(details)
    onInclusionPreviewRequested: rule => { inclusionPlan = JSON.parse(submission.preview_inclusion(rule)) }
    onInclusionPresetsRequested: save => {
        const result = JSON.parse(specimenData.inclusion_presets(save))
        presetError = result.error || ""
        if (!result.error) inclusionPresets = result
    }
    onEditRequested: edit => submission.edit(edit)
    onPublicationRequested: (title, authors) => submission.set_publication(title, authors)
    onDecisionRequested: (record, decision, accession) => submission.set_decision(record, decision, accession)
    onFrameRequested: (record, codonStart) => submission.set_frame(record, codonStart)
    onConfirmDetectedRequested: submission.confirm_detected()
    onUndoRequested: submission.undo()
    onRedoRequested: submission.redo()
    onExportRequested: submission.export_package()
    onValidateRequested: app.validateSubmission()
    onCancelValidationRequested: validationJobs.cancel()
    onAccessionsMatchRequested: text => {
        const plan = submission.plan_accessions(text)
        submissionDialog.accessionPlan = plan.length ? JSON.parse(plan) : null
    }
    onAccessionsRecordRequested: accessions => submission.record_accessions(accessions)
    onAccessionTableRequested: accessionTableDialog.dialog().open()
}
