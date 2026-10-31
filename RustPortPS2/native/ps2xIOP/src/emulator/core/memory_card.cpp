#include "memory_card.h"

#include <algorithm>
#include <fstream>
#include <iostream>
#include <map>
#include <mutex>

namespace ps2x::iop::detail
{
    namespace
    {
        // GOW-Port: each card's file is kept open. Opening and closing the file on every page write
        // (528 bytes) cost ~4.6 ms on Windows (antivirus on close): formatting and saving an 8 MB card took
        // tens of seconds with the game frozen; with the file open and one flush per page it is ~0.03 ms.
        // (memory_card.h is not changed so as not to recompile what includes it.)
        std::map<std::string, std::fstream> &openCardFiles()
        {
            static std::map<std::string, std::fstream> files;
            return files;
        }

        std::mutex &openCardFilesMutex()
        {
            static std::mutex mutex;
            return mutex;
        }

        void closeCardFile(const std::string &path)
        {
            std::lock_guard<std::mutex> lock(openCardFilesMutex());
            openCardFiles().erase(path);
        }
    }

    void MemoryCard::insert(std::string path)
    {
        closeCardFile(m_path);
        m_path = std::move(path);
        closeCardFile(m_path);
        m_present = true;
        m_loaded = false;
        m_fileExists = false;
        m_image.clear();
        m_address = 0;
    }

    void MemoryCard::eject()
    {
        closeCardFile(m_path);
        m_present = false;
        m_loaded = false;
        m_image.clear();
    }

    void MemoryCard::load()
    {
        if (m_loaded)
            return;
        m_loaded = true;
        m_image.assign(ImageSize, 0xFFu); // unformatted
        if (m_path.empty())
            return;
        std::ifstream file(m_path, std::ios::binary);
        if (!file)
            return;
        m_fileExists = true;
        file.read(reinterpret_cast<char *>(m_image.data()), static_cast<std::streamsize>(m_image.size()));
    }

    const std::vector<uint8_t> &MemoryCard::image()
    {
        load();
        return m_image;
    }

    void MemoryCard::persist(uint32_t offset, size_t size)
    {
        if (m_path.empty() || size == 0u)
            return;
        if (!m_fileExists)
        {
            // First write: the file is created with the whole image
            closeCardFile(m_path);
            std::ofstream file(m_path, std::ios::binary | std::ios::trunc);
            if (!file)
            {
                std::cerr << "[SIO2] cannot create the memory card" << m_path << std::endl;
                return;
            }
            file.write(reinterpret_cast<const char *>(m_image.data()), static_cast<std::streamsize>(m_image.size()));
            m_fileExists = static_cast<bool>(file);
            std::cerr << "[SIO2] memory card created at" << m_path << std::endl;
            if (m_fileExists)
            {
                // GOW-Port: leaves the card in the same persistent state as later writes.
                // That way a concurrent creation does not leave the first fstream with a stale descriptor.
                file.close();
            }
            return;
        }
        std::lock_guard<std::mutex> lock(openCardFilesMutex());
        std::fstream &file = openCardFiles()[m_path];
        if (!file.is_open())
        {
            file.clear();
            file.open(m_path, std::ios::binary | std::ios::in | std::ios::out);
            if (!file)
                return;
        }
        file.clear();
        file.seekp(offset);
        file.write(reinterpret_cast<const char *>(m_image.data() + offset), static_cast<std::streamsize>(size));
        file.flush(); // reaches the operating system on every write, as before on closing the file
        if (!file)
        {
            std::cerr << "[SIO2] error writing the memory card" << m_path << " offset=" << offset
                      << " bytes=" << size << std::endl;
            file.clear();
        }
    }

    void MemoryCard::read(uint8_t *destination, size_t size)
    {
        load();
        for (size_t i = 0; i < size; ++i)
        {
            const uint32_t offset = m_address + static_cast<uint32_t>(i);
            destination[i] = offset < ImageSize ? m_image[offset] : 0xFFu;
        }
        m_address += static_cast<uint32_t>(size);
    }

    void MemoryCard::write(const uint8_t *source, size_t size)
    {
        load();
        const uint32_t start = m_address;
        if (start >= ImageSize)
        {
            m_address += static_cast<uint32_t>(size);
            return;
        }
        const size_t count = std::min<size_t>(size, ImageSize - start);
        for (size_t i = 0; i < count; ++i)
            m_image[start + i] &= source[i];
        persist(start, count);
        m_address += static_cast<uint32_t>(size);
    }

    void MemoryCard::eraseBlock()
    {
        load();
        const uint32_t start = m_address;
        if (start >= ImageSize)
            return;
        const size_t count = std::min<size_t>(RawPageSize * PagesPerBlock, ImageSize - start);
        std::fill_n(m_image.begin() + start, count, uint8_t{0xFFu});
        persist(start, count);
    }

    void MemoryCardSlots::setPathProvider(std::function<std::string(uint32_t slot)> provider)
    {
        m_pathProvider = std::move(provider);
        m_prepared.fill(false);
    }

    void MemoryCardSlots::reset()
    {
        m_prepared.fill(false);
        for (MemoryCard &card : m_cards)
            card.eject();
    }

    MemoryCard *MemoryCardSlots::card(uint32_t slot)
    {
        slot &= 1u;
        if (!m_prepared[slot] && m_pathProvider)
        {
            m_prepared[slot] = true;
            const std::string path = m_pathProvider(slot);
            if (!path.empty())
            {
                m_cards[slot].insert(path);
                std::cerr << "[memory card] slot " << (slot + 1u) << ": " << path << std::endl;
            }
        }
        return m_cards[slot].present() ? &m_cards[slot] : nullptr;
    }
}
