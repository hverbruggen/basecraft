#include "instanceview.h"

#include <memory>

#include <QtGui/QFont>
#include <QtGui/QGuiApplication>
#include <QtGui/QKeyEvent>
#include <QtGui/QMouseEvent>
#include <QtGui/QImage>
#include <QtGui/QMatrix4x4>
#include <QtGui/QPainter>
#include <QtQuick/QQuickWindow>
#include <qpa/qwindowsysteminterface.h>
#include <rhi/qrhi.h>

#include "basecraft_shaders.h"

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
    void rebindAtlas();

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
    }
    if (m_pipeline)
        return;

    m_quad.reset(m_rhi->newBuffer(QRhiBuffer::Immutable, QRhiBuffer::VertexBuffer, sizeof(QuadCorners)));
    m_quad->create();
    m_quadUploaded = false;
    m_instances.reset();
    m_capacity = 0;
    m_dataDirty = true;

    m_uniforms.reset(m_rhi->newBuffer(QRhiBuffer::Dynamic, QRhiBuffer::UniformBuffer, 64));
    m_uniforms->create();

    m_sampler.reset(m_rhi->newSampler(QRhiSampler::Linear, QRhiSampler::Linear, QRhiSampler::None,
                                      QRhiSampler::ClampToEdge, QRhiSampler::ClampToEdge));
    m_sampler->create();

    m_atlas.reset(m_rhi->newTexture(QRhiTexture::RGBA8, QSize(AtlasCols * AtlasCell, AtlasRows * AtlasCell)));
    m_atlas->create();
    m_atlasDirty = true;

    m_bindings.reset(m_rhi->newShaderResourceBindings());
    rebindAtlas();

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
    m_pipeline->create();
}

void InstanceRenderer::rebindAtlas()
{
    m_bindings->setBindings({
        QRhiShaderResourceBinding::uniformBuffer(0, QRhiShaderResourceBinding::VertexStage, m_uniforms.get()),
        QRhiShaderResourceBinding::sampledTexture(1, QRhiShaderResourceBinding::FragmentStage, m_atlas.get(),
                                                  m_sampler.get()),
    });
    m_bindings->create();
}

void InstanceRenderer::synchronize(QQuickRhiItem *rhiItem)
{
    auto *item = static_cast<InstanceView *>(rhiItem);
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
    QRhiResourceUpdateBatch *updates = m_rhi->nextResourceUpdateBatch();
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
        if (bytes > m_capacity || !m_instances) {
            m_capacity = qMax<quint32>(bytes * 2, 64 * 1024);
            m_instances.reset(m_rhi->newBuffer(QRhiBuffer::Dynamic, QRhiBuffer::VertexBuffer, m_capacity));
            m_instances->create();
        }
        if (bytes > 0)
            updates->updateDynamicBuffer(m_instances.get(), 0, bytes, m_data.constData());
        m_dataDirty = false;
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
