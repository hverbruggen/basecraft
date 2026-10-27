#include "instanceview.h"

#include <memory>
#include <limits>
#include <cstring>
#include <QtCore/QPointer>
#include <QtCore/QBuffer>
#include <QtGui/QImageReader>
#include <stdexcept>

#include <QtGui/QFont>
#include <QtGui/QGuiApplication>
#include <QtGui/QKeyEvent>
#include <QtGui/QMouseEvent>
#include <QtGui/QImage>
#include <QtGui/QMatrix4x4>
#include <QtGui/QPainter>
#include <QtGui/QSurfaceFormat>
#include <QtQuick/QQuickWindow>
#include <qpa/qwindowsysteminterface.h>
#include <rhi/qrhi.h>

#include "basecraft_shaders.h"

void configure_canvas_graphics()
{
    // QRhi deliberately ignores instancing extensions on a GL 2 context, even
    // when the driver supports newer GL. This must run before QGuiApplication.
    auto format = QSurfaceFormat::defaultFormat();
#if QT_CONFIG(opengles2)
    format.setRenderableType(QSurfaceFormat::OpenGLES);
    format.setVersion(3, 0);
#else
    format.setRenderableType(QSurfaceFormat::OpenGL);
    format.setVersion(3, 3);
    format.setProfile(QSurfaceFormat::CoreProfile);
#endif
    QSurfaceFormat::setDefaultFormat(format);
}

QByteArray render_figure_png(const QString &svg, int width)
{
    constexpr qint64 maxPixels = 40'000'000;
    if (width < 1 || width > 10'000)
        throw std::runtime_error("PNG width is out of range");
    QByteArray source = svg.toUtf8();
    QBuffer input(&source);
    if (!input.open(QIODevice::ReadOnly))
        throw std::runtime_error("Could not read SVG figure");
    QImageReader reader(&input, "svg");
    const QSize original = reader.size();
    if (!original.isValid() || original.width() < 1 || original.height() < 1)
        throw std::runtime_error("SVG figure has invalid dimensions or Qt SVG support is unavailable");
    const qint64 height = (qint64(original.height()) * width + original.width() / 2) / original.width();
    if (height < 1 || height > 10'000 || height * width > maxPixels)
        throw std::runtime_error("PNG figure exceeds the supported image size");
    reader.setScaledSize(QSize(width, int(height)));
    QImage image = reader.read();
    if (image.isNull() || image.width() != width || image.height() != height)
        throw std::runtime_error("Qt could not render the SVG figure as PNG");
    QByteArray png;
    QBuffer output(&png);
    if (!output.open(QIODevice::WriteOnly) || !image.save(&output, "PNG"))
        throw std::runtime_error("Could not encode PNG figure");
    return png;
}

namespace {

constexpr int FloatsPerInstance = 16;
constexpr int AtlasCols = 16;
constexpr int AtlasRows = 6;
constexpr int AtlasCell = 48;  // pixels; glyphs are scaled down when drawn

const float QuadCorners[] = { 0, 0, 1, 0, 0, 1, 1, 1 };

QShader loadShader(const unsigned char *data, size_t size)
{
    return QShader::fromSerialized(
        QByteArray::fromRawData(reinterpret_cast<const char *>(data), qsizetype(size)));
}

// Printable ASCII in a 16 x 6 grid, white with coverage in alpha.
QImage buildAtlas(const QString &family)
{
    QImage image(AtlasCols * AtlasCell, AtlasRows * AtlasCell, QImage::Format_RGBA8888_Premultiplied);
    image.fill(Qt::transparent);
    QPainter p(&image);
    p.setRenderHint(QPainter::TextAntialiasing);
    QFont font(family);
    font.setStyleHint(QFont::Monospace);
    font.setPixelSize(int(AtlasCell * 0.8));
    p.setFont(font);
    p.setPen(Qt::white);
    for (int i = 0; i < AtlasCols * AtlasRows - 1; ++i) {
        const QRect cell((i % AtlasCols) * AtlasCell, (i / AtlasCols) * AtlasCell, AtlasCell, AtlasCell);
        p.drawText(cell, Qt::AlignCenter, QString(QChar(32 + i)));
    }
    // The last cell (127) is a filled circle, for node symbols.
    const int last = AtlasCols * AtlasRows - 1;
    const QRectF disc((last % AtlasCols) * AtlasCell + 1.5, (last / AtlasCols) * AtlasCell + 1.5,
                      AtlasCell - 3.0, AtlasCell - 3.0);
    p.setRenderHint(QPainter::Antialiasing);
    p.setPen(Qt::NoPen);
    p.setBrush(Qt::white);
    p.drawEllipse(disc);
    return image;
}

}  // namespace

class InstanceRenderer : public QQuickRhiItemRenderer
{
public:
    void initialize(QRhiCommandBuffer *cb) override;
    void synchronize(QQuickRhiItem *item) override;
    void render(QRhiCommandBuffer *cb) override;

private:
    bool rebindAtlas();
    void fail(const QString &message);
    void publishError();
    QPointer<InstanceView> m_item;
    QString m_error;
    bool m_ready = false;

    QRhi *m_rhi = nullptr;
    std::unique_ptr<QRhiBuffer> m_quad;
    std::unique_ptr<QRhiBuffer> m_instances;
    std::unique_ptr<QRhiBuffer> m_uniforms;
    std::unique_ptr<QRhiTexture> m_atlas;
    std::unique_ptr<QRhiSampler> m_sampler;
    std::unique_ptr<QRhiShaderResourceBindings> m_bindings;
    std::unique_ptr<QRhiGraphicsPipeline> m_pipeline;

    QByteArray m_data;
    bool m_dataDirty = false;
    quint32 m_capacity = 0;
    int m_count = 0;
    bool m_quadUploaded = false;

    QString m_fontFamily;
    QImage m_atlasImage;
    bool m_atlasDirty = true;

    QColor m_clearColor;
    QSizeF m_logicalSize;
};

InstanceView::InstanceView(QQuickItem *parent)
    : QQuickRhiItem(parent)
{
    setActiveFocusOnTab(true);
}

QQuickRhiItemRenderer *InstanceView::createRenderer()
{
    return new InstanceRenderer;
}

bool InstanceView::saveWindow(const QString &path)
{
    return window() && window()->grabWindow().save(path);
}

void InstanceView::typeText(const QString &text)
{
    if (!window())
        return;
    for (const QChar c : text) {
        const int key = c == u'\t' ? int(Qt::Key_Tab) : c.isLetter() ? c.toUpper().unicode() : c.unicode();
        QKeyEvent press(QEvent::KeyPress, key, Qt::NoModifier, QString(c));
        QGuiApplication::sendEvent(window(), &press);
        QKeyEvent release(QEvent::KeyRelease, key, Qt::NoModifier, QString(c));
        QGuiApplication::sendEvent(window(), &release);
    }
}

void InstanceView::pressKey(int key, int modifiers, const QString &text)
{
    if (!window())
        return;
    // The platform's entry point, as a real keyboard: shortcut handling
    // sees the press first.
    const auto mods = Qt::KeyboardModifiers(modifiers);
    QWindowSystemInterface::handleKeyEvent<QWindowSystemInterface::SynchronousDelivery>(
        window(), QEvent::KeyPress, key, mods, text);
    QWindowSystemInterface::handleKeyEvent<QWindowSystemInterface::SynchronousDelivery>(
        window(), QEvent::KeyRelease, key, mods, text);
}

void InstanceView::clickAt(qreal x, qreal y)
{
    if (!window())
        return;
    const QPointF p(x, y);
    const QPointF g = window()->mapToGlobal(p);
    QMouseEvent press(QEvent::MouseButtonPress, p, g, Qt::LeftButton, Qt::LeftButton, Qt::NoModifier);
    QGuiApplication::sendEvent(window(), &press);
    QMouseEvent release(QEvent::MouseButtonRelease, p, g, Qt::LeftButton, Qt::NoButton, Qt::NoModifier);
    QGuiApplication::sendEvent(window(), &release);
}

void InstanceView::setRenderError(const QString &error)
{
    if (m_renderError == error) return;
    m_renderError = error;
    emit renderErrorChanged();
}

void InstanceRenderer::publishError()
{
    if (!m_item) return;
    const auto item = m_item;
    const auto error = m_error;
    QMetaObject::invokeMethod(item, [item, error]() {
        if (item) item->setRenderError(error);
    }, Qt::QueuedConnection);
}

void InstanceRenderer::fail(const QString &message)
{
    m_ready = false;
    m_error = message;
    publishError();
    qWarning() << "Basecraft canvas:" << message;
}

void InstanceView::setInstances(const QByteArray &data)
{
    m_instances = data;
    emit instancesChanged();
    update();
}

void InstanceView::setClearColor(const QColor &color)
{
    if (color == m_clearColor)
        return;
    m_clearColor = color;
    emit clearColorChanged();
    update();
}

void InstanceView::setFontFamily(const QString &family)
{
    if (family == m_fontFamily)
        return;
    m_fontFamily = family;
    emit fontFamilyChanged();
    update();
}

void InstanceRenderer::initialize(QRhiCommandBuffer *)
{
    if (m_rhi != rhi()) {
        m_pipeline.reset();
        m_rhi = rhi();
        m_ready = false;
        m_error.clear();
    }
    if (m_ready)
        return;
    if (!m_error.isEmpty()) return;
    if (!m_rhi || !renderTarget()) { fail(QStringLiteral("No graphics device is available.")); return; }

    if (qEnvironmentVariableIsSet("BASECRAFT_RENDER_DIAGNOSTICS"))
        qInfo() << "BASECRAFT_RENDER instancing" << m_rhi->isFeatureSupported(QRhi::Instancing);
    if (!m_rhi->isFeatureSupported(QRhi::Instancing)) {
        fail(QStringLiteral("This graphics backend does not support instanced drawing. Select a supported graphics backend."));
        return;
    }
    m_quad.reset(m_rhi->newBuffer(QRhiBuffer::Immutable, QRhiBuffer::VertexBuffer, sizeof(QuadCorners)));
    if (!m_quad->create()) { fail(QStringLiteral("Could not create the vertex buffer.")); return; }
    m_quadUploaded = false;
    m_instances.reset();
    m_capacity = 0;
    m_dataDirty = true;

    m_uniforms.reset(m_rhi->newBuffer(QRhiBuffer::Dynamic, QRhiBuffer::UniformBuffer, 64));
    if (!m_uniforms->create()) { fail(QStringLiteral("Could not create the uniform buffer.")); return; }

    m_sampler.reset(m_rhi->newSampler(QRhiSampler::Linear, QRhiSampler::Linear, QRhiSampler::None,
                                      QRhiSampler::ClampToEdge, QRhiSampler::ClampToEdge));
    if (!m_sampler->create()) { fail(QStringLiteral("Could not create the texture sampler.")); return; }

    m_atlas.reset(m_rhi->newTexture(QRhiTexture::RGBA8, QSize(AtlasCols * AtlasCell, AtlasRows * AtlasCell)));
    if (!m_atlas->create()) { fail(QStringLiteral("Could not create the glyph atlas.")); return; }
    m_atlasDirty = true;

    m_bindings.reset(m_rhi->newShaderResourceBindings());
    if (!rebindAtlas()) { fail(QStringLiteral("Could not bind the glyph atlas.")); return; }

    m_pipeline.reset(m_rhi->newGraphicsPipeline());
    QRhiGraphicsPipeline::TargetBlend blend;
    blend.enable = true;
    blend.srcColor = QRhiGraphicsPipeline::One;
    blend.dstColor = QRhiGraphicsPipeline::OneMinusSrcAlpha;
    blend.srcAlpha = QRhiGraphicsPipeline::One;
    blend.dstAlpha = QRhiGraphicsPipeline::OneMinusSrcAlpha;
    m_pipeline->setTargetBlends({ blend });
    m_pipeline->setTopology(QRhiGraphicsPipeline::TriangleStrip);
    m_pipeline->setShaderStages({
        { QRhiShaderStage::Vertex, loadShader(basecraft_instance_vert, sizeof(basecraft_instance_vert)) },
        { QRhiShaderStage::Fragment, loadShader(basecraft_instance_frag, sizeof(basecraft_instance_frag)) },
    });
    QRhiVertexInputLayout layout;
    layout.setBindings({
        { 2 * sizeof(float) },
        { FloatsPerInstance * sizeof(float), QRhiVertexInputBinding::PerInstance },
    });
    layout.setAttributes({
        { 0, 0, QRhiVertexInputAttribute::Float2, 0 },
        { 1, 1, QRhiVertexInputAttribute::Float4, 0 },
        { 1, 2, QRhiVertexInputAttribute::Float4, 4 * sizeof(float) },
        { 1, 3, QRhiVertexInputAttribute::Float4, 8 * sizeof(float) },
        { 1, 4, QRhiVertexInputAttribute::Float4, 12 * sizeof(float) },
    });
    m_pipeline->setVertexInputLayout(layout);
    m_pipeline->setSampleCount(renderTarget()->sampleCount());
    m_pipeline->setShaderResourceBindings(m_bindings.get());
    m_pipeline->setRenderPassDescriptor(renderTarget()->renderPassDescriptor());
    if (!m_pipeline->create()) { fail(QStringLiteral("Could not create the graphics pipeline.")); return; }
    m_ready = true;
    publishError();
}

bool InstanceRenderer::rebindAtlas()
{
    m_bindings->setBindings({
        QRhiShaderResourceBinding::uniformBuffer(0, QRhiShaderResourceBinding::VertexStage, m_uniforms.get()),
        QRhiShaderResourceBinding::sampledTexture(1, QRhiShaderResourceBinding::FragmentStage, m_atlas.get(),
                                                  m_sampler.get()),
    });
    return m_bindings->create();
}

void InstanceRenderer::synchronize(QQuickRhiItem *rhiItem)
{
    auto *item = static_cast<InstanceView *>(rhiItem);
    m_item = item;
    if (item->renderError() != m_error) publishError();
    if (item->m_instances.constData() != m_data.constData() || item->m_instances.size() != m_data.size()) {
        m_data = item->m_instances;  // implicitly shared, no copy
        m_dataDirty = true;
    }
    if (item->m_fontFamily != m_fontFamily) {
        m_fontFamily = item->m_fontFamily;
        m_atlasDirty = true;
    }
    m_clearColor = item->m_clearColor;
    m_logicalSize = item->size();
}

void InstanceRenderer::render(QRhiCommandBuffer *cb)
{
    if (!m_ready || !m_rhi || !renderTarget()) return;
    constexpr qsizetype stride = FloatsPerInstance * sizeof(float);
    if (m_data.size() % stride != 0 || m_data.size() > std::numeric_limits<int>::max()) {
        fail(QStringLiteral("The canvas data exceeds the supported buffer size or has an invalid record length."));
        return;
    }
    if (m_dataDirty && (quint32(m_data.size()) > m_capacity || !m_instances)) {
        const auto bytes = quint32(m_data.size());
        // QRhi buffer sizes are signed int. Never double past their range.
        m_capacity = qMax<quint32>(bytes, 64 * 1024);
        m_instances.reset(m_rhi->newBuffer(QRhiBuffer::Dynamic, QRhiBuffer::VertexBuffer, int(m_capacity)));
        if (!m_instances->create()) { fail(QStringLiteral("Could not allocate the canvas buffer.")); return; }
    }
    QRhiResourceUpdateBatch *updates = m_rhi->nextResourceUpdateBatch();
    if (!updates) { fail(QStringLiteral("Could not allocate graphics updates.")); return; }
    if (!m_quadUploaded) {
        updates->uploadStaticBuffer(m_quad.get(), QuadCorners);
        m_quadUploaded = true;
    }
    if (m_atlasDirty) {
        m_atlasImage = buildAtlas(m_fontFamily);
        updates->uploadTexture(m_atlas.get(), m_atlasImage);
        m_atlasDirty = false;
    }
    if (m_dataDirty) {
        const quint32 bytes = quint32(m_data.size());
        m_count = int(bytes / (FloatsPerInstance * sizeof(float)));
        if (bytes > 0)
            updates->updateDynamicBuffer(m_instances.get(), 0, bytes, m_data.constData());
        m_dataDirty = false;
    }

    if (qEnvironmentVariableIsSet("BASECRAFT_RENDER_DIAGNOSTICS")) {
        float maxX = 0, maxY = 0;
        for (qsizetype offset = 0; offset + stride <= m_data.size(); offset += stride) {
            float corners[8];
            std::memcpy(corners, m_data.constData() + offset, sizeof(corners));
            for (int corner = 0; corner < 4; ++corner) {
                maxX = qMax(maxX, corners[corner * 2]);
                maxY = qMax(maxY, corners[corner * 2 + 1]);
            }
        }
        qInfo() << "BASECRAFT_RENDER" << m_logicalSize << renderTarget()->pixelSize()
                << "instances" << m_count << "extent" << maxX << maxY;
    }

    QMatrix4x4 mvp = m_rhi->clipSpaceCorrMatrix();
    mvp.ortho(0, float(m_logicalSize.width()), float(m_logicalSize.height()), 0, -1, 1);
    updates->updateDynamicBuffer(m_uniforms.get(), 0, 64, mvp.constData());

    cb->beginPass(renderTarget(), m_clearColor, { 1.0f, 0 }, updates);
    if (m_count > 0) {
        const QSize px = renderTarget()->pixelSize();
        cb->setGraphicsPipeline(m_pipeline.get());
        cb->setViewport({ 0, 0, float(px.width()), float(px.height()) });
        cb->setShaderResources();
        const QRhiCommandBuffer::VertexInput inputs[] = { { m_quad.get(), 0 }, { m_instances.get(), 0 } };
        cb->setVertexInput(0, 2, inputs);
        cb->draw(4, quint32(m_count));
    }
    cb->endPass();
}
