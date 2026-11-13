#include "releasecheck.h"

#include <QtCore/QEventLoop>
#include <QtCore/QUrl>
#include <QtNetwork/QNetworkAccessManager>
#include <QtNetwork/QNetworkReply>
#include <QtNetwork/QNetworkRequest>
#include <memory>

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
    if (!reply->isFinished())
        loop.exec();
    const int status = reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).toInt();
    if (reply->error() != QNetworkReply::NoError || status != 200)
        return QString();
    return QString::fromUtf8(reply->readAll());
}
