#include "releasecheck.h"

#include <QtCore/QEventLoop>
#include <QtCore/QUrl>
#include <QtNetwork/QNetworkAccessManager>
#include <QtNetwork/QNetworkReply>
#include <QtNetwork/QNetworkRequest>
#include <memory>
#include <mutex>

namespace {
std::mutex guard;
bool cancelled = false;
QEventLoop *waiting = nullptr;
}

QString fetchLatestRelease(const QString &url)
{
    QNetworkAccessManager manager;
    QNetworkRequest request{QUrl(url)};
    request.setTransferTimeout(10000);
    request.setHeader(QNetworkRequest::UserAgentHeader, QStringLiteral("Basecraft"));
    request.setRawHeader("Accept", "application/vnd.github+json");
    std::unique_ptr<QNetworkReply> reply{manager.get(request)};
    QEventLoop loop;
    QObject::connect(reply.get(), &QNetworkReply::finished, &loop, &QEventLoop::quit);
    {
        const std::lock_guard<std::mutex> lock(guard);
        if (cancelled)
            return QString();
        waiting = &loop;
    }
    // A quit queued by cancelReleaseCheck() before exec() still ends it.
    if (!reply->isFinished())
        loop.exec();
    {
        const std::lock_guard<std::mutex> lock(guard);
        waiting = nullptr;
        if (cancelled)
            return QString();
    }
    const int status = reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).toInt();
    if (reply->error() != QNetworkReply::NoError || status != 200)
        return QString();
    return QString::fromUtf8(reply->readAll());
}

void cancelReleaseCheck()
{
    const std::lock_guard<std::mutex> lock(guard);
    cancelled = true;
    if (waiting)
        QMetaObject::invokeMethod(waiting, &QEventLoop::quit, Qt::QueuedConnection);
}
