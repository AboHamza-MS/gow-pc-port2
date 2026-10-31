// GOW-Port: real memory and simulated fences, without a driver or game assets.
#include "MiniTest.h"
#include "gs/gs_gpu_upload_ring.h"

#include <algorithm>
#include <array>
#include <limits>
#include <set>
#include <vector>

namespace
{
    using GSGpuUpload::Failure;
    using GSGpuUpload::WaitResult;
    using TestRing = GSGpuUpload::Ring<uint32_t, 64u, 4u, 16u>;

    struct Fixture
    {
        std::array<uint8_t, 256u> bytes{};
        TestRing ring;
        uint32_t nextFence = 1u;
        bool nullFence = false;
        WaitResult outcome = WaitResult::Signaled;
        std::vector<uint32_t> waited, deleted;

        Fixture() { bytes.fill(0xa5u); ring.setStorage(bytes.data()); }
        size_t put(const void *data, size_t size)
        {
            return ring.put(data, size,
                            [&] { return nullFence ? 0u : nextFence++; },
                            [&](uint32_t fence) { waited.push_back(fence); return outcome; },
                            [&](uint32_t fence) { deleted.push_back(fence); });
        }
        void fill(TestCase &t)
        {
            for (uint8_t chunk = 0u; chunk < 4u; ++chunk)
            {
                std::array<uint8_t, 64u> data{};
                data.fill(static_cast<uint8_t>(0x10u + chunk));
                t.Equals(put(data.data(), data.size()), size_t{chunk} * 64u, "Fills four distinct chunks");
            }
        }
        void release()
        {
            ring.releaseFences([&](uint32_t fence) { deleted.push_back(fence); });
        }
    };

    void failedReuse(TestCase &t, WaitResult outcome, Failure expected)
    {
        Fixture f;
        f.fill(t);
        const auto protectedBytes = f.bytes;
        f.outcome = outcome;
        const std::array<uint8_t, 7u> data = {1u, 2u, 3u, 4u, 5u, 6u, 7u};
        std::vector<uint8_t> fallback;
        unsigned fallbacks = 0u;
        auto upload = [&]
        {
            const size_t offset = f.put(data.data(), data.size());
            if (offset == SIZE_MAX)
            {
                ++fallbacks;
                fallback.assign(data.begin(), data.end());
            }
            return offset;
        };
        t.Equals(upload(), SIZE_MAX, "Does not reuse a chunk whose fence was not signaled");
        t.Equals(f.bytes, protectedBytes, "The first failure does not alter pending uploads");
        t.IsTrue(f.ring.disabled(), "The persistent path is left disabled");
        t.IsFalse(f.ring.available(), "available also prevents reusing the ring");
        t.Equals(f.ring.failure(), expected, "Keeps the synchronization reason");
        t.Equals(upload(), SIZE_MAX, "The second upload also uses the fallback");
        t.Equals(f.bytes, protectedBytes, "Does not write at the previous failure's cursor");
        t.Equals(fallbacks, 2u, "Both uploads reach the fallback");
        t.Equals(fallback, std::vector<uint8_t>(data.begin(), data.end()), "The fallback keeps the payload");
        t.Equals(f.waited, std::vector<uint32_t>({1u}), "Does not wait again after being disabled");
        t.IsTrue(f.deleted.empty(), "Keeps the pending fence and the newly created one");
        t.Equals(f.nextFence, 5u, "Does not create a new fence on the second attempt");
        f.release();
        t.Equals(f.deleted, std::vector<uint32_t>({1u, 2u, 3u, 4u}), "Closing releases all retained handles");
        f.release();
        t.Equals(f.deleted.size(), size_t{4}, "The second close does not duplicate deletions");
    }
}

void runGpuUploadRingTests()
{
    MiniTest::Case("GS protected persistent uploads", [](TestCase &tc)
    {
        tc.Run("Timeout keeps pending memory and uses the fallback for later uploads", [](TestCase &t)
        {
            failedReuse(t, WaitResult::Timeout, Failure::Timeout);
        });
        tc.Run("WaitFailed keeps pending memory and uses the fallback for later uploads", [](TestCase &t)
        {
            failedReuse(t, WaitResult::Failed, Failure::WaitFailed);
        });
        tc.Run("A signaled fence allows recycling only the confirmed chunk", [](TestCase &t)
        {
            Fixture f;
            f.fill(t);
            const auto before = f.bytes;
            std::array<uint8_t, 64u> data{};
            data.fill(0x7bu);
            t.Equals(f.put(data.data(), data.size()), size_t{0}, "Returns to the first chunk");
            t.IsTrue(std::equal(data.begin(), data.end(), f.bytes.begin()), "Copies the payload after the confirmation");
            t.IsTrue(std::equal(before.begin() + 64u, before.end(), f.bytes.begin() + 64u), "The other three chunks do not change");
            t.Equals(f.waited, std::vector<uint32_t>({1u}), "Waits for the destination's fence");
            t.Equals(f.deleted, std::vector<uint32_t>({1u}), "Releases only the signaled fence");
            t.IsTrue(f.ring.available(), "Success keeps the ring available");
            t.Equals(f.ring.failure(), Failure::None, "Does not record a failure");
        });
        tc.Run("A fence signaled after waiting protects memory until the callback", [](TestCase &t)
        {
            Fixture f;
            f.fill(t);
            const auto before = f.bytes;
            std::array<uint8_t, 64u> data{};
            data.fill(0xebu);
            unsigned simulatedPolls = 0u;
            bool completed = false;
            const size_t offset = f.ring.put(data.data(), data.size(),
                [&] { return f.nextFence++; },
                [&](uint32_t fence)
                {
                    t.Equals(fence, 1u, "Waits for the correct chunk");
                    // GOW-Port: model the driver's bounded wait without sleeping the suite.
                    for (; simulatedPolls < 3u; ++simulatedPolls)
                        t.Equals(f.bytes, before, "Does not copy while the fence is still pending");
                    completed = true;
                    return WaitResult::Signaled;
                },
                [&](uint32_t fence)
                {
                    t.IsTrue(completed, "Only deletes the fence after the signal");
                    t.Equals(f.bytes, before, "The copy does not precede the deletion either");
                    f.deleted.push_back(fence);
                });
            t.Equals(simulatedPolls, 3u, "The late signal was modeled");
            t.Equals(offset, size_t{0}, "Accepts the confirmation after waiting");
            t.IsTrue(std::equal(data.begin(), data.end(), f.bytes.begin()), "Finally writes the payload");
            t.IsTrue(f.ring.available(), "A correct wait does not disable the ring");
        });
        tc.Run("A null fence disables without copying or touching previous uploads", [](TestCase &t)
        {
            Fixture f;
            std::array<uint8_t, 64u> data{};
            data.fill(0x27u);
            t.Equals(f.put(data.data(), data.size()), size_t{0}, "Primer chunk sin fence");
            const auto before = f.bytes;
            f.nullFence = true;
            t.Equals(f.put(data.data(), 1u), SIZE_MAX, "Does not advance without a protection fence");
            t.Equals(f.ring.failure(), Failure::NullFence, "Records the failed creation");
            t.IsTrue(f.ring.disabled(), "Is left disabled");
            t.Equals(f.bytes, before, "Does not copy after the failed creation");
            t.Equals(f.put(data.data(), 1u), SIZE_MAX, "The next upload also fails");
            t.Equals(f.bytes, before, "The second attempt keeps all the bytes");
            t.IsTrue(f.waited.empty(), "Does not wait for a nonexistent fence");
            f.release(); f.release();
            t.IsTrue(f.deleted.empty(), "Does not try to delete null handles");
        });
        tc.Run("Closing deletes the remaining handles only once", [](TestCase &t)
        {
            Fixture f;
            f.fill(t);
            std::array<uint8_t, 64u> data{};
            t.Equals(f.put(data.data(), data.size()), size_t{0}, "Recycles a chunk before closing");
            f.release(); f.release();
            t.Equals(f.deleted, std::vector<uint32_t>({1u, 2u, 3u, 4u}), "One deletion per created fence");
            t.Equals(std::set<uint32_t>(f.deleted.begin(), f.deleted.end()).size(), size_t{4}, "Does not delete a recycled handle twice");
        });
        tc.Run("Minimum padding and alignment do not write in gaps or outside the ring", [](TestCase &t)
        {
            std::array<uint8_t, 272u> guarded{};
            guarded.fill(0xa5u);
            GSGpuUpload::Ring<uint32_t, 64u, 4u, 32u> ring;
            ring.setStorage(guarded.data() + 8u);
            const std::array<uint8_t, 17u> first = {1u, 2u, 3u, 4u, 5u, 6u, 7u, 8u, 9u, 10u, 11u, 12u, 13u, 14u, 15u, 16u, 17u};
            const uint8_t one = 0x7du;
            uint32_t nextFence = 0u;
            auto put = [&](const void *data, size_t size)
            {
                return ring.put(data, size, [&] { return ++nextFence; },
                                [](uint32_t) { return WaitResult::Signaled; }, [](uint32_t) {});
            };
            t.Equals(put(first.data(), first.size()), size_t{0}, "First payload at offset zero");
            t.Equals(put(&one, 1u), size_t{32}, "Aligns the next reservation to 32 bytes");
            t.Equals(put(first.data(), first.size()), size_t{64}, "Moves the payload that does not fit to the next chunk");
            t.IsTrue(std::equal(first.begin(), first.end(), guarded.begin() + 8u), "Initial payload intact");
            for (size_t i = 17u; i < 32u; ++i)
                t.Equals(guarded[8u + i], uint8_t{0xa5u}, "Does not fill the alignment gap");
            t.Equals(guarded[8u + 32u], one, "Copies the useful byte");
            for (size_t i = 33u; i < 48u; ++i)
                t.Equals(guarded[8u + i], uint8_t{0u}, "Completes the minimum 16-byte reservation with zeros");
            for (size_t i = 48u; i < 64u; ++i)
                t.Equals(guarded[8u + i], uint8_t{0xa5u}, "Does not overwrite the end of the previous chunk");
            for (size_t i = 0u; i < 8u; ++i)
            {
                t.Equals(guarded[i], uint8_t{0xa5u}, "Initial guard intact");
                t.Equals(guarded[264u + i], uint8_t{0xa5u}, "Final guard intact");
            }
        });
        tc.Run("Without storage and with an oversized payload nothing is copied or fences created", [](TestCase &t)
        {
            TestRing ring;
            std::array<uint8_t, 256u> storage{};
            storage.fill(0xa5u);
            const auto before = storage;
            const uint8_t data = 1u;
            unsigned callbacks = 0u;
            auto put = [&](size_t size)
            {
                return ring.put(&data, size, [&] { ++callbacks; return 1u; },
                                [&](uint32_t) { ++callbacks; return WaitResult::Signaled; },
                                [&](uint32_t) { ++callbacks; });
            };
            t.IsFalse(ring.available(), "Without a mapping there is no ring available");
            t.Equals(put(1u), SIZE_MAX, "Uses the fallback before mapping");
            ring.setStorage(storage.data());
            t.Equals(put(65u), SIZE_MAX, "Does not accept an upload larger than a chunk");
            t.Equals(put((std::numeric_limits<size_t>::max)()), SIZE_MAX, "Does not overflow when computing the size");
            t.Equals(storage, before, "Rejections keep all the memory");
            t.Equals(callbacks, 0u, "Rejections do not query synchronization");
            t.IsTrue(ring.available(), "A large payload does not disable the ring");
            t.Equals(put(1u), size_t{0}, "Rejections did not advance the cursor");
        });
        tc.Run("A null source reserves without writing and size zero uses the minimum", [](TestCase &t)
        {
            Fixture f;
            const auto before = f.bytes;
            t.Equals(f.put(nullptr, 0u), size_t{0}, "Empty 16-byte reservation");
            t.Equals(f.bytes, before, "The null source keeps the reserved bytes");
            const uint8_t data = 0x55u;
            t.Equals(f.put(&data, 0u), size_t{16}, "The next reservation starts after the minimum");
            for (size_t i = 16u; i < 32u; ++i)
                t.Equals(f.bytes[i], uint8_t{0u}, "A valid source and zero payload initialize the padding");
        });
        tc.Run("A payload that ends exactly at the chunk does not create a fence too early", [](TestCase &t)
        {
            Fixture f;
            std::array<uint8_t, 48u> data{};
            t.Equals(f.put(data.data(), 15u), size_t{0}, "Minimum of 16 bytes");
            t.Equals(f.put(data.data(), data.size()), size_t{16}, "Ends exactly at the limit");
            t.Equals(f.nextFence, 1u, "Does not create a fence while the upload fits");
            t.Equals(f.put(data.data(), 1u), size_t{64}, "The next upload opens another chunk");
            t.Equals(f.nextFence, 2u, "Protects the closed chunk with a fence");
            t.IsTrue(f.waited.empty(), "Does not wait for a chunk not yet used");
        });
        tc.Run("Changing storage does not re-arm the ring after a failure", [](TestCase &t)
        {
            Fixture f;
            f.fill(t);
            f.outcome = WaitResult::Timeout;
            const uint8_t data = 0x66u;
            t.Equals(f.put(&data, 1u), SIZE_MAX, "Causes the synchronization failure");
            std::array<uint8_t, 256u> replacement{};
            replacement.fill(0x37u);
            const auto before = replacement;
            f.ring.setStorage(nullptr); f.ring.setStorage(replacement.data());
            t.Equals(f.put(&data, 1u), SIZE_MAX, "The failure remains when storage is reassigned");
            t.Equals(replacement, before, "Does not write to a reassigned mapping");
            t.Equals(f.ring.failure(), Failure::Timeout, "Keeps the original reason");
        });
        tc.Run("Pointer-type handles follow the same ownership cycle", [](TestCase &t)
        {
            std::array<uint8_t, 128u> storage{};
            std::array<uint8_t, 64u> data{};
            std::array<uint8_t, 2u> handles{};
            GSGpuUpload::Ring<const uint8_t *, 64u, 2u, 16u> ring;
            ring.setStorage(storage.data());
            size_t created = 0u;
            std::vector<const uint8_t *> waited, deleted;
            auto put = [&]
            {
                return ring.put(data.data(), data.size(),
                                [&]() -> const uint8_t * { return &handles[created++]; },
                                [&](const uint8_t *fence) { waited.push_back(fence); return WaitResult::Signaled; },
                                [&](const uint8_t *fence) { deleted.push_back(fence); });
            };
            t.Equals(put(), size_t{0}, "First upload with pointer handles");
            t.Equals(put(), size_t{64}, "Second chunk with a pointer fence");
            t.Equals(put(), size_t{0}, "Waits before returning to the first chunk");
            t.Equals(waited, std::vector<const uint8_t *>({&handles[0]}), "Queries the correct fence");
            ring.releaseFences([&](const uint8_t *fence) { deleted.push_back(fence); });
            ring.releaseFences([&](const uint8_t *fence) { deleted.push_back(fence); });
            t.Equals(deleted, std::vector<const uint8_t *>({&handles[0], &handles[1]}), "Releases each pointer once");
        });
    });
}
