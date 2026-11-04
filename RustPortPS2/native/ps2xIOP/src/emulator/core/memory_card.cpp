#include "memory_card.h"

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <map>
#include <mutex>
#include <set>
#include <system_error>

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

        // GOW-Port (R3): cards already copied to <card>.bak in this session (guarded by openCardFilesMutex).
        std::set<std::string> &backedUpCards()
        {
            static std::set<std::string> cards;
            return cards;
        }

        // GOW-Port (R3): copies through a temporary file and a rename, so a crash never leaves a half-written copy.
        bool copyThroughTemporary(const std::filesystem::path &from, const std::filesystem::path &to)
        {
            std::error_code error;
            std::filesystem::path temporary = to;
            temporary += ".tmp";
            std::filesystem::copy_file(from, temporary, std::filesystem::copy_options::overwrite_existing, error);
            if (!error)
                std::filesystem::rename(temporary, to, error);
            if (error)
            {
                std::filesystem::remove(temporary, error);
                return false;
            }
            return true;
        }

        // GOW-Port (R3): a card file of the wrong size (cut, empty, or another card format) is never written over: it is
        // moved aside as <card>.unusable[N] and the game sees a blank card it can format.
        void moveUnusableCardAside(const std::string &path, uintmax_t size)
        {
            std::error_code error;
            std::filesystem::path aside = path + ".unusable";
            for (int n = 2; std::filesystem::exists(aside, error) && n < 100; ++n)
                aside = path + ".unusable" + std::to_string(n);
            std::filesystem::rename(path, aside, error);
            std::cerr << "[memory card] " << path << " has " << size << " bytes instead of " << MemoryCard::ImageSize
                      << ": " << (error ? "left untouched" : "moved to " + aside.string())
                      << "; the game gets a blank card" << std::endl;
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
        std::error_code sizeError;
        const uintmax_t size = std::filesystem::file_size(m_path, sizeError);
        if (!sizeError && size != ImageSize)
        {
            moveUnusableCardAside(m_path, size);
            return;
        }
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
            // GOW-Port (R3): written to <card>.tmp and renamed, so a crash while creating it leaves no cut card.
            const std::string temporary = m_path + ".tmp";
            std::ofstream file(temporary, std::ios::binary | std::ios::trunc);
            if (!file)
            {
                std::cerr << "[SIO2] cannot create the memory card " << m_path << std::endl;
                return;
            }
            file.write(reinterpret_cast<const char *>(m_image.data()), static_cast<std::streamsize>(m_image.size()));
            file.close();
            std::error_code error;
            if (file)
                std::filesystem::rename(temporary, m_path, error);
            m_fileExists = file && !error;
            if (!m_fileExists)
                std::filesystem::remove(temporary, error);
            std::cerr << "[SIO2] memory card " << (m_fileExists ? "created at " : "could not be created at ") << m_path
                      << std::endl;
            {
                std::lock_guard<std::mutex> lock(openCardFilesMutex());
                backedUpCards().insert(m_path); // nothing older to keep
            }
            return;
        }
        std::lock_guard<std::mutex> lock(openCardFilesMutex());
        // GOW-Port (R3): before the first write of a session, the card as it was is kept in <card>.bak, so a save cut by
        // a crash or a power loss can be undone by hand. Once per card per session: ~9 MB, a few milliseconds.
        if (backedUpCards().insert(m_path).second)
        {
            openCardFiles().erase(m_path);
            const bool copied = copyThroughTemporary(m_path, m_path + ".bak");
            std::cerr << "[memory card] " << (copied ? "backup kept in " : "could not write the backup ") << m_path
                      << ".bak" << std::endl;
        }
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
