// GOW-Port: adaptado de Taylor N. Albarnaz / LightVelox (GPL-3.0).
// Origen: PS2Recomp sotc-port, ac9efa070638ad3b3accd284de6f898d5ab271d1.
#pragma once

#include <cstdint>

namespace GSSharedPresent
{
    struct Frame
    {
        uint32_t texture = 0;
        uint32_t width = 0;
        uint32_t height = 0;
        uint64_t sequence = 0;
        uint64_t renderSequence = 0;
        uint64_t contentHash = 0;
        bool hashValid = false;
        // GOW-Port: keep the CRTC selection without downloading the image to the host.
        uint32_t displayFbp = 0;
        uint32_t sourceFbp = 0;
        bool usedPreferred = false;
        // GOW-Port: the diagnostics use this image's tick, not a later request.
        uint64_t vsyncTick = 0;
        // GOW-Port: PMODE off is equivalent to an empty PresentationFrame for the host.
        bool blank = false;
    };

    // GOW-Port: why the GS renderer could not start, for the host to explain on screen. Written once by the GS thread
    // (GSHwBackend::ensure), read by the host thread every frame; a failed renderer never recovers.
    enum class RendererFailure : uint8_t
    {
        None,
        NoContext, // the GL context could not be created, or its entry points are missing
        Shaders,   // the driver rejected the shaders (compile or link)
    };
    void SetRendererFailure(RendererFailure failure);
    RendererFailure GetRendererFailure();

    void CaptureHostContext();
    void RestoreHostContext();
    bool Active();
    bool Acquire(Frame &frame);
    void ShutdownHost();
}
