// GOW-Port: PCSX2-compatible texture replacement packs (see runtime/gs/gs_texture_replace.h).
#include "runtime/gs/gs_texture_replace.h"

#include "runtime/gs/gs_swizzle.h"
#include "runtime/gs/ps2_gs_memory.h"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#endif

#include <algorithm>
#include <atomic>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <filesystem>
#include <list>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <unordered_set>

// Single-header libraries, compiled into this file only (stb_image would otherwise clash with raylib's copy).
#define XXH_INLINE_ALL
#include "xxhash.h"
#define BCDEC_STATIC
#define BCDEC_IMPLEMENTATION
#include "bcdec.h"
#define STB_IMAGE_STATIC
#define STB_IMAGE_IMPLEMENTATION
#define STBI_ONLY_PNG
#define STBI_NO_STDIO
#include "stb_image.h"

namespace gs_texrepl
{
    namespace
    {
        constexpr uint32_t kCT32 = 0x00, kCT24 = 0x01, kCT16 = 0x02, kCT16S = 0x0A, kT8 = 0x13, kT4 = 0x14, kT8H = 0x1B, kT4HL = 0x24, kT4HH = 0x2C;

        // What PCSX2's psm_t says of a format: block size in texels, palette entries (0 = none), whether every bit of a
        // texel belongs to the texture, and the colour's size in bits.
        struct PsmInfo
        {
            uint32_t bw = 8, bh = 8, pal = 0;
            bool full = false;
            uint32_t colour = 32;
            bool known = false;
        };

        PsmInfo formatOf(uint32_t psm)
        {
            switch (psm)
            {
            case kCT32: return {8, 8, 0, true, 32, true};
            case kCT24: return {8, 8, 0, false, 24, true};
            case kCT16: case kCT16S: return {16, 8, 0, false, 16, true};
            case kT8: return {16, 16, 256, true, 32, true};
            case kT4: return {32, 16, 16, true, 32, true};
            case kT8H: return {8, 8, 256, false, 32, true};
            case kT4HL: case kT4HH: return {8, 8, 16, false, 32, true};
            default: return {};
            }
        }

        uint32_t bitsOf(uint64_t value, int low, int count) { return static_cast<uint32_t>((value >> low) & ((1ull << count) - 1)); }

        // The part of the texture CLAMP's region modes read, on each axis that has one (PCSX2's SourceRegion::Create).
        bool hasRegionOf(uint32_t tw, uint32_t th, uint64_t clamp)
        {
            const uint32_t wms = bitsOf(clamp, 0, 2), wmt = bitsOf(clamp, 2, 2);
            const uint32_t minu = bitsOf(clamp, 4, 10), maxu = bitsOf(clamp, 14, 10);
            const uint32_t minv = bitsOf(clamp, 24, 10), maxv = bitsOf(clamp, 34, 10);
            if (wms == 2 && maxu >= minu && (maxu - minu + 1 < tw || maxu >= tw))
                return true;
            if (wms == 3 && minu != 0)
            {
                const uint32_t rw = ((minu | maxu) - maxu) + 1;
                if (rw < tw || (maxu != 0 && rw <= tw))
                    return true;
            }
            if (wmt == 2 && maxv >= minv && (maxv - minv + 1 < th || maxv >= th))
                return true;
            if (wmt == 3 && minv != 0)
            {
                const uint32_t rh = ((minv | maxv) - maxv) + 1;
                if (rh < th || (maxv != 0 && rh <= th))
                    return true;
            }
            return false;
        }

        // A texel as PCSX2 expands it for hashing: RGBA with R in the low byte, alpha from TEXA for 24- and 16-bit formats.
        uint32_t expand(uint32_t colourBits, uint32_t raw, const GSTexaReg &texa)
        {
            const uint32_t ta0 = texa.ta0, ta1 = texa.ta1;
            const bool aem = texa.aem;
            if (colourBits == 24)
                return (raw & 0xFFFFFFu) | ((aem && (raw & 0xFFFFFFu) == 0u) ? 0u : ta0 << 24);
            if (colourBits == 16)
            {
                const uint32_t rgb = (raw & 31u) << 3 | ((raw >> 5) & 31u) << 11 | ((raw >> 10) & 31u) << 19;
                const uint32_t a = (raw & 0x8000u) ? ta1 : (aem && raw == 0u) ? 0u : ta0;
                return rgb | a << 24;
            }
            return raw;
        }

        // PCSX2's HashTextureLevel for level 0 of a texture without region.
        uint64_t textureHash(const uint8_t *vram, const GSTex0Reg &t, const GSTexaReg &texa, const PsmInfo &f, uint32_t tw, uint32_t th)
        {
            const bool expanded = tw < f.bw || th < f.bh || !f.full;
            if (!expanded)
            {
                // The blocks as GS memory holds them, a row of blocks at a time.
                const GSSwizzle::Format &format = GSSwizzle::GetFormat(t.psm);
                if (!format.table)
                    return 0;
                XXH3_state_t state;
                XXH3_64bits_reset(&state);
                const uint32_t right = (tw + f.bw - 1) / f.bw * f.bw, bottom = (th + f.bh - 1) / f.bh * f.bh;
                for (uint32_t y = 0; y < bottom; y += f.bh)
                    for (uint32_t x = 0; x < right; x += f.bw)
                    {
                        const GSSwizzle::Location at = GSSwizzle::Locate(format, t.tbp0, t.tbw, x, y);
                        const uint32_t block = (at.byte >> 8) & 0x3FFFu;
                        XXH3_64bits_update(&state, vram + static_cast<size_t>(block) * 256u, 256u);
                    }
                return XXH3_64bits_digest(&state);
            }
            // Expanded: the indices a byte each, or RGBA a word each, rows one after the other.
            const bool palettized = f.pal != 0;
            std::vector<uint8_t> bytes(static_cast<size_t>(tw) * th * (palettized ? 1u : 4u));
            std::vector<uint32_t> row(tw);
            for (uint32_t y = 0; y < th; ++y)
            {
                if (palettized)
                {
                    GSMem::ReadIndexRun(vram, t.psm, t.tbp0, t.tbw, 0u, y, tw, bytes.data() + static_cast<size_t>(y) * tw);
                }
                else
                {
                    if (!GSMem::ReadColorRun(vram, t.psm, t.tbp0, t.tbw, 0u, y, tw, row.data()))
                        return 0;
                    uint32_t *out = reinterpret_cast<uint32_t *>(bytes.data()) + static_cast<size_t>(y) * tw;
                    for (uint32_t x = 0; x < tw; ++x)
                        out[x] = expand(f.colour, row[x], texa);
                }
            }
            return XXH3_64bits(bytes.data(), bytes.size());
        }

        bool parseHex(const std::string &s, uint64_t &out)
        {
            if (s.empty() || s.size() > 16)
                return false;
            uint64_t v = 0;
            for (char ch : s)
            {
                int d;
                if (ch >= '0' && ch <= '9') d = ch - '0';
                else if (ch >= 'a' && ch <= 'f') d = ch - 'a' + 10;
                else if (ch >= 'A' && ch <= 'F') d = ch - 'A' + 10;
                else return false;
                v = v << 4 | static_cast<uint64_t>(d);
            }
            out = v;
            return true;
        }

        // ---- image decoding --------------------------------------------------------------------------------------------
        uint32_t le32(const uint8_t *p) { return uint32_t(p[0]) | uint32_t(p[1]) << 8 | uint32_t(p[2]) << 16 | uint32_t(p[3]) << 24; }

        void finishImage(Image &image)
        {
            uint32_t lo = 255, hi = 0;
            const size_t pixels = static_cast<size_t>(image.width) * image.height;
            for (size_t i = 0; i < pixels; ++i)
            {
                const uint32_t a = image.data[i * 4 + 3];
                lo = std::min(lo, a);
                hi = std::max(hi, a);
            }
            image.minAlpha = static_cast<uint8_t>(pixels ? lo : 0);
            image.maxAlpha = static_cast<uint8_t>(pixels ? hi : 255);
        }

        enum BlockFormat { kRaw = 0, kBc1 = 1, kBc2, kBc3, kBc7, kBgra = -1 };

        void decodeBlocks(const uint8_t *src, uint32_t w, uint32_t h, int format, uint32_t *dst)
        {
            const uint32_t bx = (w + 3) / 4, by = (h + 3) / 4;
            const size_t blockBytes = format == kBc1 ? 8 : 16;
            uint32_t tile[16];
            for (uint32_t y = 0; y < by; ++y)
                for (uint32_t x = 0; x < bx; ++x)
                {
                    const uint8_t *block = src + (static_cast<size_t>(y) * bx + x) * blockBytes;
                    if (format == kBc1) bcdec_bc1(block, tile, 16);
                    else if (format == kBc2) bcdec_bc2(block, tile, 16);
                    else if (format == kBc3) bcdec_bc3(block, tile, 16);
                    else bcdec_bc7(block, tile, 16);
                    for (uint32_t j = 0; j < 4 && y * 4 + j < h; ++j)
                        for (uint32_t i = 0; i < 4 && x * 4 + i < w; ++i)
                            dst[static_cast<size_t>(y * 4 + j) * w + x * 4 + i] = tile[j * 4 + i];
                }
        }
    }

    std::string Name::text() const
    {
        char buffer[96];
        int n = std::snprintf(buffer, sizeof buffer, "%llx", static_cast<unsigned long long>(texture));
        if (clut)
            n += std::snprintf(buffer + n, sizeof buffer - n, "-%llx", static_cast<unsigned long long>(clut));
        if (regionW || regionH)
            n += std::snprintf(buffer + n, sizeof buffer - n, "-r%ux%u", regionW, regionH);
        std::snprintf(buffer + n, sizeof buffer - n, "-%08x", bits);
        return buffer;
    }

    bool parseName(const std::string &stem, Name &out)
    {
        std::vector<std::string> parts;
        size_t start = 0;
        for (size_t i = 0; i <= stem.size(); ++i)
            if (i == stem.size() || stem[i] == '-')
            {
                parts.push_back(stem.substr(start, i - start));
                start = i + 1;
            }
        if (parts.size() < 2 || parts.size() > 4)
            return false;
        for (const std::string &part : parts)
            if (part.empty())
                return false;
        Name n;
        uint64_t value = 0;
        if (!parseHex(parts[0], n.texture) || !parseHex(parts.back(), value) || parts.back().size() != 8)
            return false;
        n.bits = static_cast<uint32_t>(value) & ~0x4000u; // TCC: PCSX2 clears it
        size_t next = 1;
        if (next + 1 < parts.size() && parts[next][0] != 'r')
        {
            if (!parseHex(parts[next], n.clut))
                return false;
            ++next;
        }
        if (next + 1 < parts.size())
        {
            const std::string &r = parts[next];
            const size_t x = r.find('x');
            if (r[0] != 'r' || x == std::string::npos)
                return false;
            char *end = nullptr;
            const unsigned long w = std::strtoul(r.c_str() + 1, &end, 10);
            if (end != r.c_str() + x)
                return false;
            const unsigned long h = std::strtoul(r.c_str() + x + 1, &end, 10);
            if (*end != '\0')
                return false;
            n.regionW = static_cast<uint32_t>(w);
            n.regionH = static_cast<uint32_t>(h);
            ++next;
        }
        if (next + 1 != parts.size())
            return false;
        out = n;
        return true;
    }

    uint32_t nameBits(const GSTex0Reg &tex0, const GSTexaReg &texa)
    {
        const PsmInfo f = formatOf(tex0.psm);
        uint32_t bits = tex0.psm | static_cast<uint32_t>(tex0.tw) << 6 | static_cast<uint32_t>(tex0.th) << 10;
        if (f.known && !f.pal && f.colour != 32)
            bits |= static_cast<uint32_t>(texa.ta0) << 15 | (texa.aem ? 1u : 0u) << 23 | static_cast<uint32_t>(texa.ta1) << 24;
        return bits;
    }

    std::string environmentFolder(const char *name)
    {
#ifdef _WIN32
        // Windows environment variables are UTF-16; getenv's ANSI bytes are not UTF-8.
        std::wstring wideName;
        for (const char *c = name; *c; ++c)
            wideName.push_back(static_cast<wchar_t>(*c));
        const wchar_t *value = _wgetenv(wideName.c_str());
        if (!value || !value[0])
            return {};
        const int bytes = WideCharToMultiByte(CP_UTF8, 0, value, -1, nullptr, 0, nullptr, nullptr);
        if (bytes <= 1)
            return {};
        std::string utf8(static_cast<size_t>(bytes), '\0');
        WideCharToMultiByte(CP_UTF8, 0, value, -1, utf8.data(), bytes, nullptr, nullptr);
        utf8.resize(static_cast<size_t>(bytes) - 1);
        return utf8;
#else
        const char *value = std::getenv(name);
        return value ? std::string(value) : std::string();
#endif
    }

    bool nameOf(const uint8_t *vram, const GSTex0Reg &tex0, uint64_t clamp, const GSTexaReg &texa, const uint32_t *palette, Name &out,
                bool &hasRegion)
    {
        hasRegion = false;
        const PsmInfo f = formatOf(tex0.psm);
        if (!vram || !f.known || tex0.tw > 10 || tex0.th > 10)
            return false;
        const uint32_t tw = 1u << tex0.tw, th = 1u << tex0.th;
        if (hasRegionOf(tw, th, clamp))
        {
            hasRegion = true;
            return false;
        }
        Name name;
        name.texture = textureHash(vram, tex0, texa, f, tw, th);
        name.bits = nameBits(tex0, texa);
        if (f.pal)
        {
            if (!palette)
                return false;
            name.clut = XXH3_64bits(palette, static_cast<size_t>(f.pal) * 4u);
        }
        out = name;
        return true;
    }

    std::shared_ptr<Image> decodePng(const uint8_t *bytes, size_t size)
    {
        int w = 0, h = 0, channels = 0;
        unsigned char *data = stbi_load_from_memory(bytes, static_cast<int>(size), &w, &h, &channels, 4);
        if (!data)
            return nullptr;
        auto image = std::make_shared<Image>();
        image->width = static_cast<uint32_t>(w);
        image->height = static_cast<uint32_t>(h);
        image->levels = 1;
        image->data.assign(data, data + static_cast<size_t>(w) * h * 4);
        stbi_image_free(data);
        // A PNG without alpha: 0x80, the GS's 1.0 (as PCSX2 gives it).
        if (channels == 3 || channels == 1)
            for (size_t i = 3; i < image->data.size(); i += 4)
                image->data[i] = 0x80;
        finishImage(*image);
        return image;
    }

    std::shared_ptr<Image> decodeDds(const uint8_t *file, size_t size)
    {
        if (size < 128 || std::memcmp(file, "DDS ", 4) != 0)
            return nullptr;
        const uint8_t *h = file + 4;
        const uint32_t height = le32(h + 8), width = le32(h + 12), flags = le32(h + 4);
        const uint32_t mips = (flags & 0x20000) ? std::max(1u, le32(h + 24)) : 1u;
        const uint8_t *pf = h + 72;
        const uint32_t pfFlags = le32(pf + 4), fourcc = le32(pf + 8), bitcount = le32(pf + 12);
        size_t offset = 128;
        int format = kRaw; // a BlockFormat, kRaw = RGBA8, kBgra = BGRA8
        if (pfFlags & 0x4)
        {
            if (fourcc == 0x31545844) format = kBc1;      // DXT1
            else if (fourcc == 0x33545844) format = kBc2; // DXT3
            else if (fourcc == 0x35545844) format = kBc3; // DXT5
            else if (fourcc == 0x30315844)                // DX10
            {
                if (size < 148)
                    return nullptr;
                const uint32_t dxgi = le32(file + 128);
                offset = 148;
                if (dxgi == 71 || dxgi == 72) format = kBc1;
                else if (dxgi == 74 || dxgi == 75) format = kBc2;
                else if (dxgi == 77 || dxgi == 78) format = kBc3;
                else if (dxgi == 98 || dxgi == 99) format = kBc7;
                else if (dxgi == 28 || dxgi == 29) format = kRaw;
                else if (dxgi == 87 || dxgi == 91) format = kBgra;
                else return nullptr;
            }
            else
                return nullptr;
        }
        else if ((pfFlags & 0x40) && bitcount == 32)
            format = le32(pf + 16) == 0x00FF0000u ? kBgra : kRaw; // RGB masks: BGRA or RGBA
        else
            return nullptr;
        if (width == 0 || height == 0 || width > 16384 || height > 16384)
            return nullptr;

        auto image = std::make_shared<Image>();
        image->width = width;
        image->height = height;
        image->levels = 0;
        uint32_t w = width, hh = height;
        for (uint32_t n = 0; n < mips && n < 15; ++n)
        {
            const size_t bytes = format > 0 ? static_cast<size_t>((w + 3) / 4) * ((hh + 3) / 4) * (format == kBc1 ? 8 : 16)
                                            : static_cast<size_t>(w) * hh * 4;
            if (offset + bytes > size)
                break; // the levels that are whole
            const size_t at = image->data.size();
            image->data.resize(at + static_cast<size_t>(w) * hh * 4);
            uint32_t *level = reinterpret_cast<uint32_t *>(image->data.data() + at);
            if (format > 0)
                decodeBlocks(file + offset, w, hh, format, level);
            else
                for (size_t i = 0; i < static_cast<size_t>(w) * hh; ++i)
                {
                    uint32_t c = le32(file + offset + i * 4);
                    if (format == kBgra)
                        c = (c & 0xFF00FF00u) | (c >> 16 & 0xFFu) | (c & 0xFFu) << 16;
                    level[i] = c;
                }
            ++image->levels;
            offset += bytes;
            if (w == 1 && hh == 1)
                break;
            w = std::max(1u, w / 2);
            hh = std::max(1u, hh / 2);
        }
        if (image->levels == 0)
            return nullptr;
        finishImage(*image);
        return image;
    }

    // ---- the pack -----------------------------------------------------------------------------------------------------
    struct Pack::Impl
    {
        std::mutex lock;
        std::condition_variable wake;
        std::unordered_map<Name, std::filesystem::path, NameHash> files;
        struct Ready
        {
            std::shared_ptr<const Image> image;
            std::list<Name>::iterator order;
        };
        std::unordered_map<Name, Ready, NameHash> ready;
        std::list<Name> readyOrder; // oldest last
        size_t readyBytes = 0, budget = size_t(768) << 20;
        std::unordered_set<Name, NameHash> failed, queued;
        std::deque<Name> queue;
        std::vector<std::thread> readers;
        std::unordered_set<uint32_t> bitsSeen; // the `bits` of the files (written by open, read lock-free after it)
        bool stop = false;

        ~Impl()
        {
            {
                std::lock_guard<std::mutex> hold(lock);
                stop = true;
            }
            wake.notify_all();
            for (std::thread &reader : readers)
                if (reader.joinable())
                    reader.join();
        }

        static std::shared_ptr<Image> load(const std::filesystem::path &path)
        {
            std::FILE *f =
#ifdef _WIN32
                _wfopen(path.wstring().c_str(), L"rb");
#else
                std::fopen(path.string().c_str(), "rb");
#endif
            if (!f)
                return nullptr;
            std::vector<uint8_t> bytes;
            std::fseek(f, 0, SEEK_END);
            const long size = std::ftell(f);
            std::fseek(f, 0, SEEK_SET);
            if (size > 0)
            {
                bytes.resize(static_cast<size_t>(size));
                if (std::fread(bytes.data(), 1, bytes.size(), f) != bytes.size())
                    bytes.clear();
            }
            std::fclose(f);
            if (bytes.empty())
                return nullptr;
            std::string ext = path.extension().string();
            std::transform(ext.begin(), ext.end(), ext.begin(), [](char ch) { return static_cast<char>(std::tolower(static_cast<unsigned char>(ch))); });
            return ext == ".dds" ? decodeDds(bytes.data(), bytes.size()) : decodePng(bytes.data(), bytes.size());
        }

        void read()
        {
            for (;;)
            {
                Name name;
                std::filesystem::path path;
                {
                    std::unique_lock<std::mutex> hold(lock);
                    wake.wait(hold, [&] { return stop || !queue.empty(); });
                    if (stop)
                        return;
                    name = queue.front();
                    queue.pop_front();
                    path = files[name];
                }
                std::shared_ptr<const Image> image = load(path);
                std::lock_guard<std::mutex> hold(lock);
                queued.erase(name);
                if (!image)
                {
                    failed.insert(name);
                    std::fprintf(stderr, "[gow-texrepl] could not read %s\n", path.string().c_str());
                    continue;
                }
                readyOrder.push_front(name);
                ready[name] = {image, readyOrder.begin()};
                readyBytes += image->data.size();
                while (readyBytes > budget && readyOrder.size() > 1)
                {
                    auto old = ready.find(readyOrder.back());
                    readyBytes -= old->second.image->data.size();
                    ready.erase(old);
                    readyOrder.pop_back();
                }
            }
        }
    };

    Pack::Pack() : m_impl(new Impl) {}
    Pack::~Pack() = default;

    size_t Pack::open(const std::string &directory)
    {
        namespace fs = std::filesystem;
        Impl &p = *m_impl;
        std::error_code error;
        std::lock_guard<std::mutex> hold(p.lock);
        p.files.clear();
        p.bitsSeen.clear();
        const fs::path root{std::u8string(reinterpret_cast<const char8_t *>(directory.data()), directory.size())};
        if (fs::is_directory(root, error))
        {
            for (fs::recursive_directory_iterator it(root, fs::directory_options::skip_permission_denied, error), end; it != end;
                 it.increment(error))
            {
                if (error)
                    break;
                if (!it->is_regular_file(error))
                    continue;
                std::string ext = it->path().extension().string();
                std::transform(ext.begin(), ext.end(), ext.begin(), [](char ch) { return static_cast<char>(std::tolower(static_cast<unsigned char>(ch))); });
                if (ext != ".dds" && ext != ".png")
                    continue;
                Name name;
                if (!parseName(it->path().stem().string(), name))
                    continue;
                if (p.files.emplace(name, it->path()).second) // the first found, as PCSX2
                    p.bitsSeen.insert(name.bits);
            }
        }
        if (!p.files.empty() && p.readers.empty())
        {
            const unsigned cores = std::thread::hardware_concurrency();
            const unsigned n = cores >= 8 ? 3 : cores >= 4 ? 2 : 1;
            for (unsigned i = 0; i < n; ++i)
                p.readers.emplace_back([&p] {
#ifdef _WIN32
                    // Below the game's threads: a view's new files a moment later, never the game's work.
                    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
#endif
                    p.read();
                });
        }
        return p.files.size();
    }

    size_t Pack::count() const
    {
        std::lock_guard<std::mutex> hold(m_impl->lock);
        return m_impl->files.size();
    }

    bool Pack::wantsBits(uint32_t bits) const { return m_impl->bitsSeen.count(bits) != 0; }

    bool Pack::has(const Name &name) const
    {
        std::lock_guard<std::mutex> hold(m_impl->lock);
        return m_impl->files.count(name) != 0;
    }

    std::shared_ptr<const Image> Pack::take(const Name &name, bool &pending)
    {
        Impl &p = *m_impl;
        pending = false;
        std::lock_guard<std::mutex> hold(p.lock);
        if (p.files.count(name) == 0 || p.failed.count(name))
            return nullptr;
        const auto found = p.ready.find(name);
        if (found != p.ready.end())
        {
            std::shared_ptr<const Image> image = found->second.image;
            p.readyBytes -= image->data.size();
            p.readyOrder.erase(found->second.order);
            p.ready.erase(found);
            return image;
        }
        pending = true;
        if (p.queued.insert(name).second)
        {
            p.queue.push_back(name);
            p.wake.notify_one();
        }
        return nullptr;
    }

    void Pack::setMemoryBudget(size_t bytes)
    {
        std::lock_guard<std::mutex> hold(m_impl->lock);
        m_impl->budget = bytes;
    }
}
