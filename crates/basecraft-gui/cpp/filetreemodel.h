// The project's files for the sidebar: Qt's QFileSystemModel (lazy
// loading, change watching, icons), trimmed to one column and given a
// root directory QML can set.

#pragma once

#include <QtCore/QString>
#include <QtGui/QFileSystemModel>
#include <QtQml/qqmlregistration.h>

class FileTreeModel : public QFileSystemModel
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString rootDir READ rootDir WRITE setRootDir NOTIFY rootDirChanged)
    Q_PROPERTY(QModelIndex rootIndex READ rootIndex NOTIFY rootDirChanged)

public:
    explicit FileTreeModel(QObject *parent = nullptr);

    QString rootDir() const { return m_root; }
    void setRootDir(const QString &dir);
    QModelIndex rootIndex() const;

    // Name only: size, type and date columns would crowd a sidebar.
    int columnCount(const QModelIndex &parent = QModelIndex()) const override;

    Q_INVOKABLE QString pathAt(const QModelIndex &index) const { return filePath(index); }
    Q_INVOKABLE bool isDirAt(const QModelIndex &index) const { return isDir(index); }

signals:
    void rootDirChanged();

private:
    QString m_root;
};
