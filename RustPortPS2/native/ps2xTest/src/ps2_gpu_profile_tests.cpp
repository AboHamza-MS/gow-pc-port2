// GOW-Port: simulated timestamps to test the profile without the game or a driver.
#include "MiniTest.h"
#include "gs/gs_gpu_profile.h"

#include <set>
#include <vector>

namespace
{
    using GSGpuProfile::Queue;
    using GSGpuProfile::Window;

    void frames(Queue &queue, unsigned count = 4u)
    {
        for (unsigned i = 0u; i < count; ++i)
            queue.endFrame();
    }

    bool sample(Queue &queue, uint32_t kind, uint32_t begin, uint32_t end,
                std::array<uint32_t, 8> info = {})
    {
        if (!queue.sampleAllowed())
            return false;
        queue.pending.push_back({kind, begin, end, info});
        return true;
    }
}

void register_ps2_gpu_profile_tests()
{
    MiniTest::Case("GS GPU deferred profile", [](TestCase &tc)
    {
        tc.Run("A busy GPU neither reads results nor recycles queries", [](TestCase &t)
        {
            Queue q;
            sample(q, 0u, 1u, 2u);
            frames(q);
            unsigned probes = 0u, reads = 0u, reports = 0u;
            q.collect([&](uint32_t id)
                      {
                          ++probes;
                          t.Equals(id, 2u, "Only queries the availability of the last timestamp");
                          return false;
                      },
                      [&](uint32_t) { ++reads; t.Fail("Premature read"); return uint64_t{0}; },
                      [&](const Window &) { ++reports; });
            t.Equals(probes, 1u, "One query, no wait loop");
            t.Equals(reads, 0u, "Does not read a pending QUERY_RESULT");
            t.Equals(reports, 0u, "Does not report an incomplete window");
            t.Equals(q.waitingWindows(), size_t{1}, "Keeps the window");
            t.IsTrue(q.freeQueries.empty(), "Does not reuse pending handles");
        });

        tc.Run("Collects ready windows in order and only once", [](TestCase &t)
        {
            Queue q;
            sample(q, 0u, 1u, 2u); q.prims = 11u; frames(q);
            sample(q, 2u, 3u, 4u); q.prims = 29u; frames(q);
            std::vector<Window> reports;
            std::vector<uint32_t> reads;
            bool secondReady = false;
            auto collect = [&]
            {
                q.collect([&](uint32_t id) { return id == 2u || secondReady; },
                          [&](uint32_t id)
                          {
                              t.IsTrue(id <= 2u || secondReady, "Does not read the second window prematurely");
                              reads.push_back(id);
                              const uint64_t values[] = {0u, 100u, 160u, 200u, 320u};
                              return values[id];
                          },
                          [&](const Window &w) { reports.push_back(w); });
            };
            collect();
            t.Equals(reports.size(), size_t{1}, "Only collects the first window");
            t.Equals(q.waitingWindows(), size_t{1}, "Keeps the second");
            t.Equals(reads, std::vector<uint32_t>({1u, 2u}), "Reads the ready timestamps once");
            secondReady = true;
            collect(); collect();
            t.Equals(reports.size(), size_t{2}, "Does not duplicate reports");
            t.Equals(reads, std::vector<uint32_t>({1u, 2u, 3u, 4u}), "Stable order");
            if (reports.size() != 2u)
                return;
            t.Equals(reports[0].ns[0], uint64_t{60}, "Tiempo raster original");
            t.Equals(reports[1].ns[2], uint64_t{120}, "Tiempo upload original");
            t.Equals(reports[0].prims, uint64_t{11}, "Counters of the first window");
            t.Equals(reports[1].prims, uint64_t{29}, "Counters of the second");
            t.Equals(reports[0].frames, uint64_t{4}, "Four original frames");
            t.Equals(reports[1].frames, uint64_t{4}, "Four following frames");
            t.Equals(q.freeQueries, std::vector<uint32_t>({1u, 2u, 3u, 4u}), "Recycles after reading");
        });

        tc.Run("Preserves metadata and counters with their timestamps", [](TestCase &t)
        {
            Queue q;
            const std::array<uint32_t, 8> info = {7u, 8u, 9u, 10u, 0xd0u, 2u, 0x120u, 0x4bu};
            sample(q, 0u, 1u, 2u, info); sample(q, 0u, 3u, 4u, info);
            q.prims = 14u; q.pairs = 18u; q.tiles = 16u;
            q.maxBin = 10u; q.sumMaxBin = 20u;
            q.reasons[3] = 2u; q.uploadHazards[1] = 3u;
            frames(q);
            t.Equals(q.prims, uint64_t{0}, "Resets active counters");
            t.Equals(q.reasons[3], uint64_t{0}, "Resets active ratios");
            q.prims = 999u; q.reasons[3] = 999u; q.uploadHazards[1] = 999u; q.nextInfo.fill(999u);
            unsigned reports = 0u;
            q.collect([](uint32_t) { return true; },
                      [](uint32_t id) { return uint64_t{id} * 50u; },
                      [&](const Window &w)
                      {
                          ++reports;
                          t.Equals(w.ns[0], uint64_t{100}, "Adds the two samples");
                          t.Equals(w.count[0], uint64_t{2}, "Counts the two samples");
                          t.Equals(w.prims, uint64_t{14}, "Does not mix new primitives");
                          t.Equals(w.pairs, uint64_t{18}, "Keeps pairs");
                          t.Equals(w.tiles, uint64_t{16}, "Keeps tiles");
                          t.Equals(w.maxBin, uint64_t{10}, "Keeps maximum");
                          t.Equals(w.sumMaxBin, uint64_t{20}, "Keeps sum");
                          t.Equals(w.reasons[3], uint64_t{2}, "Keeps ratios");
                          t.Equals(w.uploadHazards[1], uint64_t{3}, "Keeps hazards");
                          t.Equals(w.rasterTimes.size(), size_t{2}, "Keeps raster samples");
                          for (const auto &entry : w.rasterTimes)
                          {
                              t.Equals(entry.first, uint64_t{50}, "Correct duration");
                              t.Equals(entry.second, info, "Original FBP, PSM, ZBP and flags");
                          }
                      });
            t.Equals(reports, 1u, "Reports the frozen window");
            t.Equals(q.prims, uint64_t{999}, "Does not alter new counters when collecting");
        });

        tc.Run("A full queue keeps frames without waiting", [](TestCase &t)
        {
            Queue q;
            for (uint32_t i = 1u; i <= 3u; ++i)
            {
                sample(q, 0u, i * 2u - 1u, i * 2u); q.prims = i; frames(q);
            }
            sample(q, 0u, 7u, 8u); q.prims = 4u; frames(q);
            t.Equals(q.waitingWindows(), GSGpuProfile::kMaxWaitingWindows, "Bounded queue");
            t.Equals(q.frames, uint64_t{4}, "Keeps the active window");
            t.Equals(q.prims, uint64_t{4}, "Keeps active counters");
            std::vector<Window> reports;
            q.collect([](uint32_t) { return false; },
                      [&](uint32_t) { t.Fail("Does not wait with a full queue"); return uint64_t{0}; },
                      [&](const Window &) { t.Fail("Does not report pending results"); });
            auto collect = [&]
            {
                q.collect([](uint32_t) { return true; },
                          [](uint32_t id) { return uint64_t{id} * 10u; },
                          [&](const Window &w) { reports.push_back(w); });
            };
            collect(); q.endFrame(); collect();
            t.Equals(q.frames, uint64_t{0}, "Seals when space is freed");
            t.Equals(reports.size(), size_t{4}, "Does not lose windows");
            uint64_t totalFrames = 0u, totalPrims = 0u;
            for (const auto &w : reports)
            {
                totalFrames += w.frames; totalPrims += w.prims;
                t.Equals(w.ns[0], uint64_t{10}, "Each window keeps its sample");
            }
            t.Equals(totalFrames, uint64_t{17}, "Counts 17 real frames");
            t.Equals(totalPrims, uint64_t{10}, "Counts 10 real primitives");
            if (reports.size() == 4u)
                t.Equals(reports[3].frames, uint64_t{5}, "Real divisor with backpressure");
        });

        tc.Run("Limits queries and marks omissions without losing counters", [](TestCase &t)
        {
            Queue q;
            for (uint32_t i = 0u; i < GSGpuProfile::kMaxSamplesPerWindow + 3u; ++i)
            {
                sample(q, 0u, i * 2u + 1u, i * 2u + 2u); ++q.prims;
            }
            t.Equals(q.pending.size(), GSGpuProfile::kMaxSamplesPerWindow, "Maximum 4096 pairs");
            frames(q);
            unsigned reports = 0u;
            q.collect([](uint32_t) { return true; },
                      [](uint32_t id) { return uint64_t{id} * 100u; },
                      [&](const Window &w)
                      {
                          ++reports;
                          t.Equals(w.skippedQueries, uint64_t{3}, "Explicitly partial profile");
                          t.Equals(w.prims, uint64_t{4099}, "Keeps work without a sample");
                          t.Equals(w.count[0], uint64_t{4096}, "Counts accepted samples");
                          t.Equals(w.ns[0], uint64_t{409600}, "Time of accepted samples");
                          t.Equals(w.rasterTimes.size(), size_t{4096}, "Bounded raster memory");
                      });
            t.Equals(reports, 1u, "Single partial report");
            t.Equals(q.freeQueries.size(), size_t{8192}, "Recycles accepted queries");
            const std::set<uint32_t> handles(q.freeQueries.begin(), q.freeQueries.end());
            t.Equals(handles.size(), size_t{8192}, "Does not recycle handles twice");
            t.IsTrue(q.sampleAllowed(), "Next window measures again");
            t.Equals(q.skippedQueries, uint64_t{0}, "Does not carry over omissions");
        });

        tc.Run("Closing collects a short window and recycles all handles", [](TestCase &t)
        {
            Queue q;
            sample(q, 0u, 1u, 2u); frames(q);
            sample(q, 5u, 3u, 4u); frames(q, 2u);
            q.finish(); q.finish();
            unsigned probes = 0u;
            std::vector<Window> reports;
            q.collect([&](uint32_t) { ++probes; return false; },
                      [](uint32_t id) { return uint64_t{id} * 10u; },
                      [&](const Window &w) { reports.push_back(w); }, true);
            t.Equals(probes, 0u, "Only closing allows a forced read");
            t.Equals(reports.size(), size_t{2}, "Closing twice does not duplicate");
            if (reports.size() == 2u)
            {
                t.Equals(reports[0].frames, uint64_t{4}, "Complete window");
                t.Equals(reports[1].frames, uint64_t{2}, "Short window divisor");
                t.Equals(reports[1].ns[5], uint64_t{10}, "Presentation timestamp");
            }
            t.Equals(q.waitingWindows(), size_t{0}, "None are left pending");
            t.Equals(q.freeQueries.size(), size_t{4}, "Handles ready for DeleteQueries");
        });

        tc.Run("An empty close does not invent frames", [](TestCase &t)
        {
            Queue q;
            q.finish();
            q.collect([](uint32_t) { return true; },
                      [&](uint32_t) { t.Fail("There are no queries"); return uint64_t{0}; },
                      [&](const Window &) { t.Fail("There are no windows"); }, true);
            t.Equals(q.waitingWindows(), size_t{0}, "Empty queue");
            t.IsTrue(q.freeQueries.empty(), "No crea handles");
        });

        tc.Run("A window without samples keeps frames and counters", [](TestCase &t)
        {
            Queue q;
            q.reasons[5] = 2u; frames(q);
            unsigned reports = 0u;
            q.collect([&](uint32_t) { t.Fail("Does not need availability"); return false; },
                      [&](uint32_t) { t.Fail("There are no timestamps"); return uint64_t{0}; },
                      [&](const Window &w)
                      {
                          ++reports;
                          t.Equals(w.frames, uint64_t{4}, "Four frames without queries");
                          t.Equals(w.reasons[5], uint64_t{2}, "Keeps flushes");
                          t.Equals(w.count[0], uint64_t{0}, "Does not invent samples");
                      });
            t.Equals(reports, 1u, "Reports real counters");
        });
    });
}
