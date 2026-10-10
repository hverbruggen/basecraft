#include "filetreemodel.h"

FileTreeModel::FileTreeModel(QObject *parent)
    : QFileSystemModel(parent)
{
    // Hidden entries (the .basecraft folder among them) stay out of view.
    setFilter(QDir::AllDirs | QDir::Files | QDir::NoDotAndDotDot);
    setReadOnly(true);
}

void FileTreeModel::setRootDir(const QString &dir)
{
    if (dir == m_root)
        return;
    m_root = dir;
    setRootPath(dir);
    emit rootDirChanged();
}

QModelIndex FileTreeModel::rootIndex() const
{
    return m_root.isEmpty() ? QModelIndex() : index(m_root);
}

int FileTreeModel::columnCount(const QModelIndex &parent) const
{
    Q_UNUSED(parent);
    return 1;
}
