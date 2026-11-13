#pragma once

#include <QtCore/QString>

// The body of a GET of `url`, for a job thread. Empty on any failure, a status
// other than 200, or no answer within 10 seconds.
QString fetchLatestRelease(const QString &url);
