#ifndef PS2_GIF_ARBITER_H
#define PS2_GIF_ARBITER_H

#include <array>
#include <cstdint>
#include <functional>
#include <vector>

enum class GifPathId : uint8_t
{
    Path1 = 1,
    Path2 = 2,
    Path3 = 3,
};

struct GifArbiterPacket
{
    GifPathId pathId;
    bool path2DirectHl = false;
    bool path3Image = false;
    // GOW-Port: the buffer came through submitBuffer (moved in, not copied); it goes to the spent list after use.
    bool spent = false;
    std::vector<uint8_t> data;
};

class GifArbiter
{
public:
    using ProcessPacketFn = std::function<void(const uint8_t *, uint32_t)>;
    // GOW-Port: identify the stream without breaking the existing two-argument callbacks.
    using ProcessPathPacketFn = std::function<void(GifPathId, const uint8_t *, uint32_t)>;

    GifArbiter() = default;
    explicit GifArbiter(ProcessPacketFn processFn);

    void setProcessPacketFn(ProcessPacketFn fn);
    void setProcessPathPacketFn(ProcessPathPacketFn fn) { m_processFn = std::move(fn); }
    void reset() { m_queue.clear(); m_spent.clear(); m_path3Input = {}; }

    void submit(GifPathId pathId, const uint8_t *data, uint32_t sizeBytes, bool path2DirectHl = false);
    // submit + drain. With nothing queued the packet is the whole batch: it goes to the callback in place, without the
    // queue's copy (every XGKICK takes this path).
    void submitAndDrain(GifPathId pathId, const uint8_t *data, uint32_t sizeBytes, bool path2DirectHl = false);
    // GOW-Port: submit without the queue's copy: the buffer is moved in, and after it is processed it waits in the spent list
    // (takeSpentBuffers) so the owner can reuse it. The GS worker hands each GIF DMA this way (idea from gow-native-pc).
    void submitBuffer(GifPathId pathId, std::vector<uint8_t> &&data, bool path2DirectHl = false);
    std::vector<std::vector<uint8_t>> takeSpentBuffers();

    void drain();
    bool empty() const { return m_queue.empty(); }

private:
    ProcessPathPacketFn m_processFn;
    std::vector<GifArbiterPacket> m_queue;
    std::vector<std::vector<uint8_t>> m_spent;

    // GOW-Port: the first quadword of a DMA block can be payload, not a GIFtag.
    struct Path3InputState
    {
        std::array<uint8_t, 16> tag{};
        uint32_t tagBytes = 0u;
        uint32_t payloadBytes = 0u;
        uint8_t format = 0u;
    } m_path3Input;

    bool trackPath3Packet(const uint8_t *data, uint32_t sizeBytes);
    static uint8_t pathPriority(GifPathId id);
};

#endif
