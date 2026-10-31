#include "runtime/gs/ps2_gif_arbiter.h"
#include <algorithm>
#include <cstring>

GifArbiter::GifArbiter(ProcessPacketFn processFn)
{
    setProcessPacketFn(std::move(processFn));
}

// GOW-Port: compatibility with clients that do not need the PATH identifier.
void GifArbiter::setProcessPacketFn(ProcessPacketFn fn)
{
    if (!fn) { m_processFn = {}; return; }
    m_processFn = [fn = std::move(fn)](GifPathId, const uint8_t *data, uint32_t size) { fn(data, size); };
}

bool GifArbiter::trackPath3Packet(const uint8_t *data, uint32_t sizeBytes)
{
    // GOW-Port: walk only the tags; skip registers, pixels and the REGLIST padding.
    // The cursor follows the PATH3 FIFO between submit/drain without retaining its payloads.
    auto &input = m_path3Input;
    bool containsImage = false;
    uint32_t offset = 0u;
    while (offset < sizeBytes)
    {
        if (input.payloadBytes)
        {
            containsImage |= input.format >= 2u; // GOW-Port: IMAGE e IMAGE2.
            const uint32_t bytes = std::min(input.payloadBytes, sizeBytes - offset);
            input.payloadBytes -= bytes;
            offset += bytes;
            continue;
        }

        const uint8_t *tag = data + offset;
        if (input.tagBytes || sizeBytes - offset < 16u)
        {
            const uint32_t bytes = std::min(16u - input.tagBytes, sizeBytes - offset);
            std::memcpy(input.tag.data() + input.tagBytes, data + offset, bytes);
            input.tagBytes += bytes;
            offset += bytes;
            if (input.tagBytes < 16u) break;
            tag = input.tag.data();
            input.tagBytes = 0u;
        }
        else
            offset += 16u;

        uint64_t lo = 0u;
        std::memcpy(&lo, tag, sizeof(lo));
        const uint32_t loops = static_cast<uint32_t>(lo & 0x7FFFu);
        input.format = static_cast<uint8_t>((lo >> 58u) & 3u);
        uint32_t nreg = static_cast<uint32_t>((lo >> 60u) & 15u);
        if (!nreg) nreg = 16u;
        if (input.format == 0u)
            input.payloadBytes = loops * nreg * 16u;
        else if (input.format == 1u)
            input.payloadBytes = ((loops * nreg + 1u) / 2u) * 16u;
        else
            input.payloadBytes = loops * 16u;
        containsImage |= input.format >= 2u && loops != 0u;
    }
    return containsImage;
}

void GifArbiter::submit(GifPathId pathId, const uint8_t *data, uint32_t sizeBytes, bool path2DirectHl)
{
    if (!data || sizeBytes < 16 || !m_processFn || pathId < GifPathId::Path1 || pathId > GifPathId::Path3)
        return;

    GifArbiterPacket pkt;
    pkt.pathId = pathId;
    pkt.path2DirectHl = (pathId == GifPathId::Path2) && path2DirectHl;
    pkt.path3Image = (pathId == GifPathId::Path3) && trackPath3Packet(data, sizeBytes);
    pkt.data.resize(sizeBytes);
    std::memcpy(pkt.data.data(), data, sizeBytes);
    m_queue.push_back(std::move(pkt));
}

// GOW-Port: submit + drain. With nothing queued the packet is the whole batch, so it goes to the callback in place without the
// queue's copy (every XGKICK takes this path). Idea from Waffle565/God-Of-War-Native-Port (docs/dev/PERFORMANCE.md, ATHN01D).
void GifArbiter::submitAndDrain(GifPathId pathId, const uint8_t *data, uint32_t sizeBytes, bool path2DirectHl)
{
    if (!m_queue.empty() || !data || sizeBytes < 16 || !m_processFn || pathId < GifPathId::Path1 || pathId > GifPathId::Path3)
    {
        submit(pathId, data, sizeBytes, path2DirectHl);
        drain();
        return;
    }
    if (pathId == GifPathId::Path3)
        trackPath3Packet(data, sizeBytes); // the PATH3 tag cursor follows every packet
    m_processFn(pathId, data, sizeBytes);
    drain(); // packets the callback submitted meanwhile
}

void GifArbiter::drain()
{
    if (!m_processFn)
        return;

    while (!m_queue.empty())
    {
        // GOW-Port: the DIRECTHL/IMAGE exception is not a strict weak order and cannot
        // be used as a stable_sort comparator. Grouping only by path keeps their FIFOs.
        std::stable_sort(m_queue.begin(), m_queue.end(),
                         [](const GifArbiterPacket &a, const GifArbiterPacket &b)
                         {
                             return pathPriority(a.pathId) < pathPriority(b.pathId);
                         });
        const size_t count = m_queue.size();
        size_t path2Begin = 0u;
        while (path2Begin < count && m_queue[path2Begin].pathId == GifPathId::Path1)
            ++path2Begin;
        size_t path2End = path2Begin;
        while (path2End < count && m_queue[path2End].pathId == GifPathId::Path2)
            ++path2End;

        for (size_t i = 0u; i < path2Begin; ++i)
        {
            auto &pkt = m_queue[i];
            if (!pkt.data.empty())
                m_processFn(pkt.pathId, pkt.data.data(), static_cast<uint32_t>(pkt.data.size()));
        }
        size_t path2 = path2Begin, path3 = path2End;
        while (path2 < path2End || path3 < count)
        {
            // GOW-Port: decide among the heads, never move a packet ahead within the path.
            // DIRECTHL waits for IMAGE at the PATH3 head; DIRECT keeps normal priority.
            const bool takePath2 = path2 < path2End &&
                (path3 == count || !m_queue[path2].path2DirectHl || !m_queue[path3].path3Image);
            auto &pkt = m_queue[takePath2 ? path2++ : path3++];
            if (!pkt.data.empty())
                m_processFn(pkt.pathId, pkt.data.data(), static_cast<uint32_t>(pkt.data.size()));
        }
        // GOW-Port: keep the packets the callback added; they are processed in another batch.
        m_queue.erase(m_queue.begin(), m_queue.begin() + count);
    }
}

uint8_t GifArbiter::pathPriority(GifPathId id)
{
    return static_cast<uint8_t>(id);
}
