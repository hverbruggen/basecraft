// A Qt Quick item that draws instanced quads through Qt's RHI.
//
// Rust computes the instances (see basecraft-viewmodel's frame.rs for the
// layout) and hands them over as a QByteArray. This item only uploads and
// draws them; it holds no editor logic.

#pragma once

#include <QtGui/QColor>
#include <QtCore/QByteArray>
#include <QtCore/QString>
#include <QtQml/qqmlregistration.h>
#include <QtQuick/QQuickRhiItem>

// Configure the minimum OpenGL context needed by the instanced RHI canvas.
void configure_canvas_graphics();

// Render an editable figure SVG through Qt's SVG image format plugin.
// Throws on missing plugin, invalid dimensions, or failed rendering.
QByteArray render_figure_png(const QString &svg, int width);

class InstanceView : public QQuickRhiItem
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString renderError READ renderError NOTIFY renderErrorChanged)
    Q_PROPERTY(QByteArray instances READ instances WRITE setInstances NOTIFY instancesChanged)
    Q_PROPERTY(QColor clearColor READ clearColor WRITE setClearColor NOTIFY clearColorChanged)
    Q_PROPERTY(QString fontFamily READ fontFamily WRITE setFontFamily NOTIFY fontFamilyChanged)

public:
    explicit InstanceView(QQuickItem *parent = nullptr);

    QQuickRhiItemRenderer *createRenderer() override;

    // Save the whole window as an image, for scripted screenshots.
    Q_INVOKABLE bool saveWindow(const QString &path);
    // Deliver key presses for `text` to the window as if typed, for
    // scripted tests of focus and key handling.
    Q_INVOKABLE void typeText(const QString &text);
    // Deliver one key press with modifiers (Qt::KeyboardModifiers bits),
    // likewise.
    Q_INVOKABLE void pressKey(int key, int modifiers, const QString &text);
    // Deliver a left click at window coordinates, likewise.
    Q_INVOKABLE void clickAt(qreal x, qreal y);

    QString renderError() const { return m_renderError; }
    void setRenderError(const QString &error);

    QByteArray instances() const { return m_instances; }
    void setInstances(const QByteArray &data);
    QColor clearColor() const { return m_clearColor; }
    void setClearColor(const QColor &color);
    QString fontFamily() const { return m_fontFamily; }
    void setFontFamily(const QString &family);

signals:
    void renderErrorChanged();
    void instancesChanged();
    void clearColorChanged();
    void fontFamilyChanged();

private:
    friend class InstanceRenderer;
    QString m_renderError;
    QByteArray m_instances;
    QColor m_clearColor = Qt::white;
    QString m_fontFamily = QStringLiteral("monospace");
};
