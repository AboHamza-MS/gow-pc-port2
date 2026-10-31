#pragma once

// GOW-Port: the memory cards as native storage objects: a raw card image (PCSX2 .ps2 layout, 528-byte pages) written
// through to its file. Used by the native memory-card routines (src/native/iop_native_overrides.cpp, docs/dev/MEMORY-CARD.md).

#include <array>
#include <cstddef>
#include <cstdint>
#include <functional>
#include <string>
#include <vector>

namespace ps2x::iop::detail
{
    class MemoryCard
    {
    public:
        static constexpr uint32_t PageSize = 512u;
        static constexpr uint32_t SpareSize = 16u;
        static constexpr uint32_t RawPageSize = PageSize + SpareSize; // 528
        static constexpr uint32_t PagesPerBlock = 16u;
        static constexpr uint32_t PageCount = 0x4000u; // 8 MB
        static constexpr uint32_t ImageSize = RawPageSize * PageCount;

        // Empty path: memory-only card (tests). If the file does not exist, the card starts unformatted (0xFF)
        // and the file is created on the first write.
        void insert(std::string path);
        void eject();
        [[nodiscard]] bool present() const noexcept { return m_present; }
        [[nodiscard]] const std::string &path() const noexcept { return m_path; }

        void setAddress(uint32_t page) noexcept { m_address = page * RawPageSize; }
        void read(uint8_t *destination, size_t size);
        void write(const uint8_t *source, size_t size); // flash only turns bits from 1 to 0 (AND)
        void eraseBlock();                              // 16 pages from the current address
        [[nodiscard]] uint32_t address() const noexcept { return m_address; }
        [[nodiscard]] const std::vector<uint8_t> &image();


    private:
        void load();
        void persist(uint32_t offset, size_t size);

        std::string m_path;
        bool m_present = false;
        bool m_loaded = false;
        bool m_fileExists = false;
        std::vector<uint8_t> m_image;
        uint32_t m_address = 0;
    };

    // The two card slots; the image file of a slot is resolved on first use (GOW_MC0 / GOW_MC1 / Mcd001.ps2).
    class MemoryCardSlots
    {
    public:
        void setPathProvider(std::function<std::string(uint32_t slot)> provider);
        void reset();
        // the card in `slot` (0/1), or nullptr when the slot is empty
        [[nodiscard]] MemoryCard *card(uint32_t slot);

    private:
        std::function<std::string(uint32_t)> m_pathProvider;
        std::array<MemoryCard, 2> m_cards{};
        std::array<bool, 2> m_prepared{};
    };
}
