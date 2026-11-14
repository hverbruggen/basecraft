import QtQuick

// A new sequence typed or pasted in; it opens in its own tab once created.
AddSequenceDialog {
    id: newSequenceDialogItem
    required property var app
    controller: app.shell
    newDocument: true
    onInputError: message => validationMessage = message
    onRejected: {
        const pending = app.pendingSequence
        app.pendingSequence = null
        if (!pending) return
        const pendingIndex = app.documentOpens.index_of(pending.token)
        if (pendingIndex >= 0) app.removeDocument(pendingIndex)
        app.tabs.currentIndex = pending.previous
    }
    onCreateRequested: (name, sequence) => {
        if (app.pendingSequence) return
        const error = app.shell.new_sequence_error(name, sequence)
        if (error.length) { validationMessage = error; return }
        const index = app.documentOpens.new_sequence(name)
        app.pendingSequence = {path: app.documentOpens.path_at(index), name: name, sequence: sequence, previous: app.tabs.currentIndex, token: app.documentOpens.token_at(index)}
        app.tabs.currentIndex = index
    }
}
