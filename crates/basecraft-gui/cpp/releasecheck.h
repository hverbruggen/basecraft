#pragma once

#include <QtCore/QString>

// The body of a GET of `url`, for a job thread. Empty on any failure, a status
// other than 200, no answer within 10 seconds, or after cancelReleaseCheck().
QString fetchLatestRelease(const QString &url);

// Stop a request still waiting, and refuse later ones; for quitting.
void cancelReleaseCheck();
