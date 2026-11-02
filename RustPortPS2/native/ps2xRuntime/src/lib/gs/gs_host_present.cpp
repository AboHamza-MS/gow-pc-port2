// GOW-Port: host context, frame selection and raylib drawing used by the port.
#include "runtime/gs/gs_host_present.h"
#include "rlgl.h"
#include "runtime/gs/gs_frontend.h"
#include "runtime/ps2_perf.h"
#include "ps2_log.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

float gowPresentationAspect();                                // GOW-Port: gow_integration.cpp
void gowFrameDraw(float x, float y, float w, float h, uint64_t presentedVsyncTick, bool hasSharedFrame); // GOW-Port: gow_integration.cpp
namespace
{
    constexpr uint32_t kWidth = 640u;
    constexpr uint32_t kHeight = 512u;
    constexpr uint32_t kDisplayHeight = 448u;
}


namespace
{
    // GOW-Port: output scaler. The GS renders at its native resolution (512x448); this pass maps that image onto the window
    // at the window's resolution with a 5-tap Catmull-Rom bicubic (GOW_SCALER=sharp, default) instead of the plain
    // bilinear stretch (GOW_SCALER=bilinear), then applies a light contrast-adaptive sharpen. It does not add detail
    // (that needs a scaled GS render target); it removes the blur of the stretch.
    const char *kScalerFragment = R"GLSL(
#version 330
in vec2 fragTexCoord;
in vec4 fragColor;
uniform sampler2D texture0;
uniform vec2 srcSize;
uniform float sharpen;
uniform float plain;
uniform float fxaa;
out vec4 finalColor;

vec4 tex(vec2 uv) { return texture(texture0, uv); }

// GOW-Port: FXAA (PC_FXAA=1, launcher) over the game's texels, before the window scaling.
vec3 fxaaColor(vec2 uv)
{
    vec2 inv = 1.0 / srcSize;
    vec3 rgbNW = tex(uv + vec2(-1.0, -1.0) * inv).rgb, rgbNE = tex(uv + vec2(1.0, -1.0) * inv).rgb;
    vec3 rgbSW = tex(uv + vec2(-1.0, 1.0) * inv).rgb, rgbSE = tex(uv + vec2(1.0, 1.0) * inv).rgb;
    vec3 rgbM = tex(uv).rgb;
    const vec3 luma = vec3(0.299, 0.587, 0.114);
    float lumaNW = dot(rgbNW, luma), lumaNE = dot(rgbNE, luma), lumaSW = dot(rgbSW, luma), lumaSE = dot(rgbSE, luma);
    float lumaM = dot(rgbM, luma);
    float lumaMin = min(lumaM, min(min(lumaNW, lumaNE), min(lumaSW, lumaSE)));
    float lumaMax = max(lumaM, max(max(lumaNW, lumaNE), max(lumaSW, lumaSE)));
    vec2 dir = vec2(-((lumaNW + lumaNE) - (lumaSW + lumaSE)), (lumaNW + lumaSW) - (lumaNE + lumaSE));
    float dirReduce = max((lumaNW + lumaNE + lumaSW + lumaSE) * (0.25 / 8.0), 1.0 / 128.0);
    float rcpDirMin = 1.0 / (min(abs(dir.x), abs(dir.y)) + dirReduce);
    dir = clamp(dir * rcpDirMin, vec2(-8.0), vec2(8.0)) * inv;
    vec3 rgbA = 0.5 * (tex(uv + dir * (1.0 / 3.0 - 0.5)).rgb + tex(uv + dir * (2.0 / 3.0 - 0.5)).rgb);
    vec3 rgbB = rgbA * 0.5 + 0.25 * (tex(uv - dir * 0.5).rgb + tex(uv + dir * 0.5).rgb);
    float lumaB = dot(rgbB, luma);
    return (lumaB < lumaMin || lumaB > lumaMax) ? rgbA : rgbB;
}

void main()
{
    if (fxaa > 0.5)
    {
        finalColor = vec4(fxaaColor(fragTexCoord), 1.0);
        return;
    }
    if (plain > 0.5)
    {
        // frame at least as large as its window rectangle: one bilinear tap, no bicubic / sharpen
        finalColor = vec4(texture(texture0, fragTexCoord).rgb, 1.0);
        return;
    }
    vec2 pos = fragTexCoord * srcSize;
    vec2 center = floor(pos - 0.5) + 0.5;
    vec2 f = pos - center;
    vec2 f2 = f * f, f3 = f2 * f;
    vec2 w0 = -0.5 * f3 + f2 - 0.5 * f;
    vec2 w1 = 1.5 * f3 - 2.5 * f2 + 1.0;
    vec2 w2 = -1.5 * f3 + 2.0 * f2 + 0.5 * f;
    vec2 w3 = 0.5 * f3 - 0.5 * f2;
    vec2 w12 = w1 + w2;
    vec2 offset12 = w2 / w12;
    vec2 tc0 = (center - 1.0) / srcSize;
    vec2 tc3 = (center + 2.0) / srcSize;
    vec2 tc12 = (center + offset12) / srcSize;
    vec4 c = tex(vec2(tc12.x, tc0.y)) * (w12.x * w0.y)
           + tex(vec2(tc0.x, tc12.y)) * (w0.x * w12.y)
           + tex(vec2(tc12.x, tc12.y)) * (w12.x * w12.y)
           + tex(vec2(tc3.x, tc12.y)) * (w3.x * w12.y)
           + tex(vec2(tc12.x, tc3.y)) * (w12.x * w3.y);
    float norm = (w12.x * w0.y) + (w0.x * w12.y) + (w12.x * w12.y) + (w3.x * w12.y) + (w12.x * w3.y);
    c /= norm;
    if (sharpen <= 0.0) // GOW-Port: sharpness 0 = only the bicubic
    {
        finalColor = vec4(clamp(c.rgb, 0.0, 1.0), 1.0);
        return;
    }
    // contrast-adaptive sharpen from the four direct neighbours of the nearest source texel
    vec2 px = 1.0 / srcSize;
    vec3 n = tex(fragTexCoord + vec2(0.0, -px.y)).rgb, s = tex(fragTexCoord + vec2(0.0, px.y)).rgb;
    vec3 e = tex(fragTexCoord + vec2(px.x, 0.0)).rgb, w = tex(fragTexCoord + vec2(-px.x, 0.0)).rgb;
    vec3 lo = min(min(n, s), min(e, w)), hi = max(max(n, s), max(e, w));
    vec3 amp = clamp(min(lo, 1.0 - hi) / max(hi, vec3(0.0001)), 0.0, 1.0);
    float peak = -1.0 / mix(8.0, 5.0, sharpen);
    vec3 wgt = sqrt(amp) * peak;
    vec3 sharp = (c.rgb + (n + s + e + w) * wgt) / (1.0 + 4.0 * wgt);
    finalColor = vec4(clamp(sharp, 0.0, 1.0), 1.0);
}
)GLSL";

    Shader g_scaler{};
    int g_scalerSrcSize = -1, g_scalerSharpen = -1, g_scalerPlain = -1;
    int g_scalerFxaa = -1; // GOW-Port: PC_FXAA
    bool g_scalerTried = false;

    bool scalerEnabled()
    {
        // GOW-Port: with PC_FXAA=1 the shader is also needed with the bilinear stretch.
        static const bool on = [] {
            const char *v = std::getenv("GOW_SCALER"), *fxaa = std::getenv("PC_FXAA");
            return !(v && std::strcmp(v, "bilinear") == 0) || (fxaa && fxaa[0] == '1');
        }();
        return on;
    }

    Shader *scalerShader()
    {
        if (!g_scalerTried)
        {
            g_scalerTried = true;
            if (scalerEnabled())
            {
                g_scaler = LoadShaderFromMemory(nullptr, kScalerFragment);
                if (g_scaler.id != 0u && g_scaler.id != rlGetShaderIdDefault())
                {
                    g_scalerSrcSize = GetShaderLocation(g_scaler, "srcSize");
                    g_scalerSharpen = GetShaderLocation(g_scaler, "sharpen");
                    g_scalerPlain = GetShaderLocation(g_scaler, "plain");
                    g_scalerFxaa = GetShaderLocation(g_scaler, "fxaa");
                }
                else
                {
                    std::fprintf(stderr, "[gs-present] scaler shader failed, using bilinear\n");
                    g_scaler = {};
                }
            }
        }
        return g_scaler.id != 0u ? &g_scaler : nullptr;
    }
}

namespace
{
    // GOW-Port: what the player reads when the GS renderer could not start (GSSharedPresent::RendererFailure). Portuguese
    // when PC_LANGUAGE starts with "pt", the same rule as the cheat menu; the accents are octal escapes of their UTF-8 bytes
    // so the file does not depend on the compiler's source charset.
    struct FailureText
    {
        const char *title;
        const char *lines[3];
    };

    const FailureText &failureText(GSSharedPresent::RendererFailure failure)
    {
        static const FailureText english[2] = {
            {"The graphics renderer could not start",
             {"This game needs a GPU and driver with OpenGL 4.3 or newer.",
              "Update your graphics driver, or run the game on a different GPU (on laptops, choose the dedicated one).",
              "Details are in last.log, in the launcher's logs folder."}},
            {"The graphics renderer could not start",
             {"The graphics driver rejected the game's shaders.", "Update your graphics driver.",
              "Details are in last.log, in the launcher's logs folder."}}};
        static const FailureText portuguese[2] = {
            {"O renderizador gr\303\241fico n\303\243o p\303\264de iniciar",
             {"Este jogo precisa de uma GPU e de um driver com OpenGL 4.3 ou mais novo.",
              "Atualize o driver da placa de v\303\255deo, ou rode o jogo em outra GPU (em notebooks, escolha a dedicada).",
              "Os detalhes est\303\243o em last.log, na pasta de logs do launcher."}},
            {"O renderizador gr\303\241fico n\303\243o p\303\264de iniciar",
             {"O driver gr\303\241fico recusou os shaders do jogo.", "Atualize o driver da placa de v\303\255deo.",
              "Os detalhes est\303\243o em last.log, na pasta de logs do launcher."}}};
        static const bool pt = [] { const char *language = std::getenv("PC_LANGUAGE"); return language && std::strncmp(language, "pt", 2) == 0; }();
        const size_t index = failure == GSSharedPresent::RendererFailure::Shaders ? 1u : 0u;
        return pt ? portuguese[index] : english[index];
    }

    // Greedy word wrap of text to maxWidth at the given size (the raylib default font).
    void wrapText(const char *text, float size, float spacing, float maxWidth, std::vector<std::string> &out)
    {
        const Font font = GetFontDefault();
        std::string line, word;
        const auto flush = [&] {
            if (!line.empty())
                out.push_back(line);
            line.clear();
        };
        for (const char *p = text;; ++p)
        {
            if (*p != ' ' && *p != '\0')
            {
                word.push_back(*p);
                continue;
            }
            if (!word.empty())
            {
                const std::string candidate = line.empty() ? word : line + " " + word;
                if (!line.empty() && MeasureTextEx(font, candidate.c_str(), size, spacing).x > maxWidth)
                {
                    flush();
                    line = word;
                }
                else
                    line = candidate;
                word.clear();
            }
            if (*p == '\0')
                break;
        }
        flush();
    }

    // An opaque dark panel over the picture area (so the magenta placeholder is not shown) and the message centered in it:
    // the title, then the lines, word-wrapped to the panel, with a size proportional to the window height that shrinks
    // until everything fits. The wrapped lines are kept until the window size changes.
    void drawRendererFailure(const Rectangle &area, float screenHeight, GSSharedPresent::RendererFailure failure)
    {
        struct Layout
        {
            float width = 0.0f, height = 0.0f;
            GSSharedPresent::RendererFailure failure = GSSharedPresent::RendererFailure::None;
            float titleSize = 0.0f, bodySize = 0.0f, gap = 0.0f;
            std::vector<std::string> title, body;
        };
        static Layout layout;
        if (layout.width != area.width || layout.height != screenHeight || layout.failure != failure)
        {
            const FailureText &text = failureText(failure);
            layout = {};
            layout.width = area.width;
            layout.height = screenHeight;
            layout.failure = failure;
            const float maxWidth = area.width * 0.86f, maxHeight = area.height * 0.86f;
            float bodySize = std::max(10.0f, screenHeight * 0.034f);
            for (;; bodySize *= 0.92f)
            {
                layout.bodySize = bodySize;
                layout.titleSize = bodySize * 1.4f;
                layout.gap = bodySize * 0.7f;
                layout.title.clear();
                layout.body.clear();
                wrapText(text.title, layout.titleSize, layout.titleSize * 0.1f, maxWidth, layout.title);
                for (const char *line : text.lines)
                    wrapText(line, bodySize, bodySize * 0.1f, maxWidth, layout.body);
                const float height = layout.title.size() * layout.titleSize * 1.2f + layout.gap +
                                     layout.body.size() * bodySize * 1.35f + layout.gap;
                if (height <= maxHeight || bodySize <= 8.0f)
                    break;
            }
        }
        const Font font = GetFontDefault();
        const float titleLine = layout.titleSize * 1.2f, bodyLine = layout.bodySize * 1.35f;
        const float total = layout.title.size() * titleLine + layout.gap + layout.body.size() * bodyLine + layout.gap;
        float y = area.y + (area.height - total) * 0.5f;
        DrawRectangleRec(area, Color{20, 20, 20, 255});
        for (const std::string &line : layout.title)
        {
            const float width = MeasureTextEx(font, line.c_str(), layout.titleSize, layout.titleSize * 0.1f).x;
            DrawTextEx(font, line.c_str(), Vector2{std::round(area.x + (area.width - width) * 0.5f), std::round(y)},
                       layout.titleSize, layout.titleSize * 0.1f, Color{255, 255, 255, 255});
            y += titleLine;
        }
        y += layout.gap;
        for (const std::string &line : layout.body)
        {
            const float width = MeasureTextEx(font, line.c_str(), layout.bodySize, layout.bodySize * 0.1f).x;
            DrawTextEx(font, line.c_str(), Vector2{std::round(area.x + (area.width - width) * 0.5f), std::round(y)},
                       layout.bodySize, layout.bodySize * 0.1f, Color{214, 214, 214, 255});
            y += bodyLine;
        }
    }
}

void GSHostPresentation::captureHostContext()
{
    if (started)
        return;
    started = true;
    const char *setting = std::getenv("PS2X_GS_DIRECT_PRESENT");
    // GOW-Port: shared GL textures (no readback/upload) by default; PS2X_GS_DIRECT_PRESENT=0 forces RAM frames.
    captureRequested = !(setting && *setting == '0');
    if (captureRequested)
        GSSharedPresent::CaptureHostContext();
}

void GSHostPresentation::restoreHostContext()
{
    if (captureRequested)
        GSSharedPresent::RestoreHostContext();
}

void GSHostPresentation::createFallback()
{
    if (fallback.id != 0u)
        return;
    Image blank = GenImageColor(kWidth, kHeight, BLANK);
    fallback = LoadTextureFromImage(blank);
    UnloadImage(blank);
    SetTextureFilter(fallback, TEXTURE_FILTER_POINT); // Coincide con la ruta RAM original.
    upload.resize(static_cast<size_t>(kWidth) * kHeight * 4u, 0u);
}

void GSHostPresentation::blankFallback()
{
    if (!uploadedBlank)
    {
        for (size_t offset = 0u; offset < upload.size(); offset += 4u)
        {
            upload[offset] = MAGENTA.r;
            upload[offset + 1u] = MAGENTA.g;
            upload[offset + 2u] = MAGENTA.b;
            upload[offset + 3u] = MAGENTA.a;
        }
        UpdateTexture(fallback, upload.data());
        uploadedBlank = true;
    }
    current.texture = fallback;
    current.width = kWidth;
    current.height = kDisplayHeight;
    current.borrowed = false;
    current.blank = true;
    uploaded = true;
}

GSHostPresentation::View GSHostPresentation::prepare(GS &gs, uint64_t vsyncTick)
{
    static const bool presentDiag = std::getenv("GOW_PRESENT_DIAG") != nullptr;
    const auto presentStart = presentDiag ? std::chrono::steady_clock::now() : std::chrono::steady_clock::time_point{};
    createFallback();
    const bool shared = gs.usesSharedPresentation() && GSSharedPresent::Active();
    if (!modeKnown || shared != sharedMode)
    {
        std::fprintf(stderr, "[gs-present] host mode: %s\n", shared ? "shared texture" : "RAM frames");
        std::fflush(stderr);
        modeKnown = true;
        sharedMode = shared;
        hasLatched = uploaded = uploadedBlank = false;
        lastSharedSequence = 0u;
        current = {};
    }
    const bool needsLatch = !hasLatched || vsyncTick != lastTick;
    if (needsLatch)
    {
        gs.latchHostPresentationFrame();
        lastTick = vsyncTick;
        hasLatched = true;
    }
    const auto afterLatch = presentDiag ? std::chrono::steady_clock::now() : std::chrono::steady_clock::time_point{};
    current.shared = shared;
    if (shared)
    {
        // GOW-Port: the previous call already finished EndDrawing, including the batch.
        // Acquire can keep the previous slot; never download pixels here.
        GSSharedPresent::Frame frame;
        const bool acquired = GSSharedPresent::Acquire(frame);
        if (presentDiag)
        {
            const auto afterAcquire = std::chrono::steady_clock::now();
            const double latchMs = std::chrono::duration<double, std::milli>(afterLatch - presentStart).count();
            const double acquireMs = std::chrono::duration<double, std::milli>(afterAcquire - afterLatch).count();
            if (latchMs + acquireMs >= 30.0)
                std::fprintf(stderr, "[gow-present-diag] tick=%llu latch_ms=%.2f acquire_ms=%.2f acquired=%d sequence=%llu\n",
                             static_cast<unsigned long long>(vsyncTick), latchMs, acquireMs, acquired ? 1 : 0,
                             acquired ? static_cast<unsigned long long>(frame.sequence) : 0ull);
        }
        if (acquired)
        {
            current.frame = frame;
            if (frame.sequence != lastSharedSequence)
            {
                gs.recordAcquiredSharedFrame(frame);
                lastSharedSequence = frame.sequence;
            }
            if (!frame.blank && frame.texture != 0u && frame.width != 0u && frame.height != 0u)
            {
                current.texture = Texture2D{frame.texture, static_cast<int>(frame.width), static_cast<int>(frame.height),
                                            1, PIXELFORMAT_UNCOMPRESSED_R8G8B8A8};
                current.width = frame.width;
                current.height = frame.height;
                current.borrowed = true;
                current.blank = false;
                return current;
            }
        }
        blankFallback(); // PMODE off keeps the magenta RAM placeholder.
        return current;
    }
    if (!needsLatch && uploaded)
        return current;

    scratch.clear();
    uint32_t width = 0u, height = 0u, displayFbp = 0u, sourceFbp = 0u;
    bool preferred = false;
    if (std::getenv("GOW_GS_HW_PROFILE"))
        std::fprintf(stderr, "[gs-host-ram-path] tick=%llu shared=%d mode_known=%d shared_mode=%d needs_latch=%d\n",
                     static_cast<unsigned long long>(vsyncTick), shared ? 1 : 0, modeKnown ? 1 : 0,
                     sharedMode ? 1 : 0, needsLatch ? 1 : 0);
    if (!gs.copyLatchedHostPresentationFrame(scratch, width, height, &displayFbp, &sourceFbp, &preferred))
    {
        blankFallback();
        lastWidth = current.width;
        lastHeight = current.height;
        return current;
    }
    PS2_IF_AGRESSIVE_LOGS({
        if (uploadDebugCount < 128u || displayFbp != lastDisplayFbp || sourceFbp != lastSourceFbp ||
            preferred != lastPreferred || width != lastWidth || height != lastHeight)
            std::cout << "[frame:upload] idx=" << uploadDebugCount << " tick=" << vsyncTick
                      << " displayFbp=" << displayFbp << " sourceFbp=" << sourceFbp
                      << " size=" << width << "x" << height
                      << " preferred=" << static_cast<uint32_t>(preferred ? 1u : 0u) << std::endl;
        ++uploadDebugCount;
    });
    lastDisplayFbp = displayFbp;
    lastSourceFbp = sourceFbp;
    lastPreferred = preferred;
    lastWidth = width;
    lastHeight = height;
    if (width > kWidth || height > kHeight)
    {
        // GOW-Port: render-target renderer frames (internal resolution) are uploaded as they are, in a texture of their size.
        if (width > fallbackWidth || height > fallbackHeight)
        {
            UnloadTexture(fallback);
            fallbackWidth = std::max(fallbackWidth, width);
            fallbackHeight = std::max(fallbackHeight, height);
            Image bigBlank = GenImageColor(static_cast<int>(fallbackWidth), static_cast<int>(fallbackHeight), BLANK);
            fallback = LoadTextureFromImage(bigBlank);
            UnloadImage(bigBlank);
            SetTextureFilter(fallback, TEXTURE_FILTER_POINT);
        }
        UpdateTextureRec(fallback, Rectangle{0.0f, 0.0f, static_cast<float>(width), static_cast<float>(height)}, scratch.data());
        uploaded = true;
        uploadedBlank = false;
        current.texture = fallback;
        current.width = width;
        current.height = height;
        current.borrowed = current.blank = false;
        current.frame = {};
        return current;
    }
    std::fill(upload.begin(), upload.end(), 0u);
    const uint32_t copyWidth = std::min(width, kWidth);
    const uint32_t copyHeight = std::min(height, kHeight);
    const size_t sourceRow = static_cast<size_t>(width) * 4u;
    const size_t destinationRow = static_cast<size_t>(kWidth) * 4u;
    const size_t copyRow = static_cast<size_t>(copyWidth) * 4u;
    for (uint32_t y = 0u; y < copyHeight; ++y)
    {
        const size_t sourceOffset = static_cast<size_t>(y) * sourceRow;
        const size_t destinationOffset = static_cast<size_t>(y) * destinationRow;
        if (sourceOffset + copyRow > scratch.size() || destinationOffset + copyRow > upload.size())
            break;
        std::memcpy(upload.data() + destinationOffset, scratch.data() + sourceOffset, copyRow);
    }
    UpdateTexture(fallback, upload.data());
    uploaded = true;
    uploadedBlank = false;
    current.texture = fallback;
    current.width = width;
    current.height = height;
    current.borrowed = current.blank = false;
    current.frame = {};
    return current;
}

GSHostPresentation::View GSHostPresentation::renderFrame(GS &gs, uint64_t vsyncTick, const std::function<void()> &overlay)
{
    static const bool pacingDiag = [] { const char *v = std::getenv("GOW_FRAME_PACING_DIAG"); return v && *v == '1'; }();
    View selected;
    {
        ps2_perf::Scope perf(ps2_perf::Bucket::Upload);
        selected = prepare(gs, vsyncTick);
    }
    BeginDrawing();
    ClearBackground(BLACK);
    const float sourceWidth = static_cast<float>(std::max<uint32_t>(1u, selected.width));
    const float sourceHeight = static_cast<float>(std::max<uint32_t>(1u, selected.height));
    const float screenWidth = static_cast<float>(GetScreenWidth());
    const float screenHeight = static_cast<float>(GetScreenHeight());
    // GOW-Port: a PS2 frame (e.g. 512x448) is shown at the TV's aspect, not with square pixels.
    // GOW-Port: PC_ASPECT / PC_WIDESCREEN (gow_integration.cpp); 0 = stretch to the whole window. Without the launcher's
    // widescreen switch the game draws its original 4:3 picture (src/game/gow_pad2.cpp), so gowPresentationAspect falls
    // back to 4:3: the window shows side bars instead of a stretched image.
    const float displayAspect = gowPresentationAspect();
    const float destinationWidth = displayAspect > 0.0f ? std::min(screenWidth, screenHeight * displayAspect) : screenWidth;
    const float destinationHeight = displayAspect > 0.0f ? destinationWidth / displayAspect : screenHeight;
    const Rectangle source{0.0f, 0.0f, sourceWidth, sourceHeight};
    const Rectangle destination{(screenWidth - destinationWidth) * 0.5f,
                                (screenHeight - destinationHeight) * 0.5f,
                                destinationWidth, destinationHeight};
    // GOW-Port: PBO texture with rows in order; do not flip source.height.
    // The bicubic + sharpen pass only helps when the frame is magnified; a frame at least as large as its window
    // rectangle (PS2X_GS_HW_SCALE high enough, or a small window) is drawn with one linear tap (same shader, "plain").
    const bool magnified = sourceWidth < destinationWidth || sourceHeight < destinationHeight;
    Shader *scaler = scalerShader();
    if (!scaler)
        SetTextureFilter(selected.texture, TEXTURE_FILTER_BILINEAR);
    if (scaler)
    {
        const float size[2] = {sourceWidth, sourceHeight};
        static const float sharpen = [] { const char *v = std::getenv("GOW_SHARPEN"); return v ? static_cast<float>(std::atof(v)) : 0.5f; }();
        SetShaderValue(*scaler, g_scalerSrcSize, size, SHADER_UNIFORM_VEC2);
        SetShaderValue(*scaler, g_scalerSharpen, &sharpen, SHADER_UNIFORM_FLOAT);
        const float plain = magnified ? 0.0f : 1.0f;
        SetShaderValue(*scaler, g_scalerPlain, &plain, SHADER_UNIFORM_FLOAT);
        static const float fxaa = [] { const char *v = std::getenv("PC_FXAA"); return v && v[0] == '1' ? 1.0f : 0.0f; }();
        SetShaderValue(*scaler, g_scalerFxaa, &fxaa, SHADER_UNIFORM_FLOAT);
        SetTextureFilter(selected.texture, TEXTURE_FILTER_BILINEAR);
        BeginShaderMode(*scaler);
    }
    DrawTexturePro(selected.texture, source, destination, Vector2{0.0f, 0.0f}, 0.0f, WHITE);
    if (scaler)
        EndShaderMode();
    gowFrameDraw(destination.x, destination.y, destination.width, destination.height,
                     selected.frame.vsyncTick, selected.shared && !selected.blank); // GOW-Port
    // GOW-Port: no renderer, so no frame will ever come: say why instead of the magenta placeholder. A blank frame without a
    // failure (PMODE off) keeps its look.
    if (selected.blank)
    {
        const GSSharedPresent::RendererFailure failure = GSSharedPresent::GetRendererFailure();
        if (failure != GSSharedPresent::RendererFailure::None)
            drawRendererFailure(destination, screenHeight, failure);
    }
    if (overlay)
        overlay();
    // GOW-Port: GOW_HOST_SHOT=<n>: at host frame n write the window's back buffer (what is about to be shown) to
    // host_shot.png in the working directory, plus the frame / window sizes, to check presentation without a desktop grab.
    // GOW_HOST_SHOT=<n>:<k> writes k consecutive frames (host_shot_0.png ... host_shot_<k-1>.png) to see flicker between frames.
    static const long hostShotAt = [] { const char *v = std::getenv("GOW_HOST_SHOT"); return v ? std::atol(v) : -1L; }();
    static const long hostShotCount = [] {
        const char *v = std::getenv("GOW_HOST_SHOT");
        const char *colon = v ? std::strchr(v, ':') : nullptr;
        return colon ? std::max(1L, std::atol(colon + 1)) : 1L;
    }();
    static long hostFrame = 0;
    const long shotIndex = hostFrame++ - hostShotAt;
    if (hostShotAt >= 0 && shotIndex >= 0 && shotIndex < hostShotCount)
    {
        rlDrawRenderBatchActive();
        Image screen = LoadImageFromScreen();
        char shotName[64];
        if (hostShotCount > 1)
            std::snprintf(shotName, sizeof(shotName), "host_shot_%ld.png", shotIndex);
        else
            std::snprintf(shotName, sizeof(shotName), "host_shot.png");
        ExportImage(screen, shotName);
        UnloadImage(screen);
        std::fprintf(stderr, "[gs-present] host_shot frame=%ux%u tex=%u window=%dx%d shared=%d scaler=%d\n", selected.width, selected.height,
                     selected.texture.id, GetScreenWidth(), GetScreenHeight(), selected.shared ? 1 : 0, scaler ? 1 : 0);
    }
    // GOW-Port: GOW_HOST_SHOT_ON_CHANGE=<k>: when the published frame's displayed buffer, size or blank state changes (a
    // gameplay <-> cutscene switch), the window's back buffer of that host frame and the next k-1 are written to
    // host_change_<n>_<i>.png (at most 8 triggers, at least 120 host frames apart). To see what the window shows right after a switch.
    {
        static const long changeFrames = [] { const char *v = std::getenv("GOW_HOST_SHOT_ON_CHANGE"); return v ? std::atol(v) : 0L; }();
        static long changeRemaining = 0, changeTriggers = 0, changeFrame = 0, changeLastTrigger = -1000;
        static uint32_t lastFbp = 0u, lastW = 0u, lastH = 0u;
        static bool lastBlank = false, haveLast = false;
        ++changeFrame;
        if (changeFrames > 0 && selected.shared)
        {
            const uint32_t fbp = selected.frame.displayFbp, w = selected.frame.width, h = selected.frame.height;
            const bool changed = haveLast && (fbp != lastFbp || w != lastW || h != lastH || selected.blank != lastBlank);
            if (changed && changeRemaining == 0 && changeTriggers < 8 && changeFrame - changeLastTrigger >= 120)
            {
                ++changeTriggers;
                changeRemaining = changeFrames;
                changeLastTrigger = changeFrame;
                std::fprintf(stderr, "[gs-present] change trigger=%ld host_frame=%ld fbp %u->%u size %ux%u->%ux%u blank %d->%d tick=%llu\n",
                             changeTriggers, changeFrame, lastFbp, fbp, lastW, lastH, w, h, lastBlank ? 1 : 0, selected.blank ? 1 : 0,
                             static_cast<unsigned long long>(selected.frame.vsyncTick));
            }
            haveLast = true;
            lastFbp = fbp, lastW = w, lastH = h, lastBlank = selected.blank;
            if (changeRemaining > 0)
            {
                rlDrawRenderBatchActive();
                Image screen = LoadImageFromScreen();
                char changeName[64];
                std::snprintf(changeName, sizeof(changeName), "host_change_%ld_%02ld.png", changeTriggers, changeFrames - changeRemaining);
                ExportImage(screen, changeName);
                UnloadImage(screen);
                std::fprintf(stderr, "[gs-present] change_shot %s fbp=%u blank=%d tick=%llu\n", changeName, fbp, selected.blank ? 1 : 0,
                             static_cast<unsigned long long>(selected.frame.vsyncTick));
                --changeRemaining;
            }
        }
    }
    const auto beforeEndDrawing = pacingDiag ? std::chrono::steady_clock::now() : std::chrono::steady_clock::time_point{};
    {
        ps2_perf::Scope perf(ps2_perf::Bucket::HostWait);
        EndDrawing();
    }
    if (pacingDiag)
    {
        struct PacingStats
        {
            std::chrono::steady_clock::time_point lastEnd{}, lastReport{};
            uint64_t lastSequence = 0, lastTick = 0, hostFrames = 0, newFrames = 0, repeatedFrames = 0;
            uint64_t newTicks = 0, repeatedTicks = 0, blankFrames = 0, skippedSequences = 0;
            std::vector<double> gapsMs, endDrawingMs;
        };
        static PacingStats stats;
        const auto now = std::chrono::steady_clock::now();
        if (stats.lastReport == std::chrono::steady_clock::time_point{})
            stats.lastReport = now;
        if (stats.lastEnd != std::chrono::steady_clock::time_point{})
            stats.gapsMs.push_back(std::chrono::duration<double, std::milli>(now - stats.lastEnd).count());
        stats.endDrawingMs.push_back(std::chrono::duration<double, std::milli>(now - beforeEndDrawing).count());
        stats.lastEnd = now;
        ++stats.hostFrames;
        if (selected.blank)
            ++stats.blankFrames;
        else if (selected.shared)
        {
            if (selected.frame.vsyncTick && selected.frame.vsyncTick != stats.lastTick)
                ++stats.newTicks, stats.lastTick = selected.frame.vsyncTick;
            else
                ++stats.repeatedTicks;
            if (selected.frame.sequence != stats.lastSequence)
            {
                if (stats.lastSequence && selected.frame.sequence > stats.lastSequence + 1u)
                    stats.skippedSequences += selected.frame.sequence - stats.lastSequence - 1u;
                ++stats.newFrames;
                stats.lastSequence = selected.frame.sequence;
            }
            else
                ++stats.repeatedFrames;
        }
        if (now - stats.lastReport >= std::chrono::seconds(5))
        {
            const auto percentile = [](std::vector<double> &values, double p)
            {
                if (values.empty()) return 0.0;
                std::sort(values.begin(), values.end());
                const size_t index = std::min(values.size() - 1u, static_cast<size_t>(std::ceil(p * values.size())) - 1u);
                return values[index];
            };
            std::fprintf(stderr, "[gow-frame-pacing] host=%llu new=%llu repeated=%llu newTicks=%llu repeatedTicks=%llu skipped=%llu blank=%llu gap_p50_ms=%.2f gap_p95_ms=%.2f gap_p99_ms=%.2f gap_max_ms=%.2f enddrawing_p95_ms=%.2f window=%dx%d\n",
                         static_cast<unsigned long long>(stats.hostFrames), static_cast<unsigned long long>(stats.newFrames),
                         static_cast<unsigned long long>(stats.repeatedFrames),
                         static_cast<unsigned long long>(stats.newTicks), static_cast<unsigned long long>(stats.repeatedTicks),
                         static_cast<unsigned long long>(stats.skippedSequences),
                         static_cast<unsigned long long>(stats.blankFrames),
                         percentile(stats.gapsMs, 0.50), percentile(stats.gapsMs, 0.95), percentile(stats.gapsMs, 0.99),
                         stats.gapsMs.empty() ? 0.0 : stats.gapsMs.back(), percentile(stats.endDrawingMs, 0.95),
                         GetScreenWidth(), GetScreenHeight());
            stats.gapsMs.clear();
            stats.endDrawingMs.clear();
            stats.hostFrames = stats.newFrames = stats.repeatedFrames = stats.newTicks = stats.repeatedTicks = stats.skippedSequences = stats.blankFrames = 0;
            stats.lastReport = now;
        }
    }
    return selected;
}

void GSHostPresentation::shutdown(GS &gs)
{
    if (!started && fallback.id == 0u)
        return;
    // GOW-Port: nullptr installs a null backend; it drains and destroys the shared worker/context.
    gs.setRasterBackend(nullptr);
    restoreHostContext();
    if (fallback.id != 0u)
        UnloadTexture(fallback); // Never unload the producer's borrowed Texture2D.
    fallback = {};
    if (captureRequested)
        GSSharedPresent::ShutdownHost();
    started = captureRequested = hasLatched = uploaded = uploadedBlank = modeKnown = sharedMode = false;
    lastTick = lastSharedSequence = 0u;
    current = {};
    scratch.clear();
    upload.clear();
}
