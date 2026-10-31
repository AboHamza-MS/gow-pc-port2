// GOW-Port: texture replacement packs in PCSX2's layout (GSTextureReplacements): a folder of .png / .dds files, each named
// for the texture it replaces, which the GS renderer draws instead of the game's texture.
//
//   <texture>[-<clut>][-r<w>x<h>]-<bits>.png|dds        hashes in hex
//   texture : XXH3-64 of the texture's blocks as GS memory holds them (or of its texels expanded to RGBA / one index
//             byte per texel: 24- and 16-bit formats, indexed formats stored in the high bits, textures smaller than a
//             block)
//   clut    : XXH3-64 of the 16 / 256 palette entries as RGBA words (only for indexed formats)
//   region  : size of the part of the texture CLAMP's region modes read (only when there is one)
//   bits    : PSM | TW << 6 | TH << 10 (TCC, bit 14, ignored), and TEXA's TA0 << 15, AEM << 23, TA1 << 24 for the 24- and
//             16-bit formats.
//
// The algorithm is PCSX2's (GPL-3.0); the structure follows ps2-port-engine's texture_pack (MIT). No GL here: the
// renderer uploads the decoded image.
#pragma once

#include "runtime/gs/gs_types.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace gs_texrepl
{
    // Which texture, as PCSX2's file names spell it.
    struct Name
    {
        uint64_t texture = 0, clut = 0;
        uint32_t regionW = 0, regionH = 0;
        uint32_t bits = 0;
        bool operator==(const Name &o) const
        {
            return texture == o.texture && clut == o.clut && regionW == o.regionW && regionH == o.regionH && bits == o.bits;
        }
        std::string text() const; // as a file is named, without its extension
    };
    struct NameHash
    {
        size_t operator()(const Name &n) const
        {
            return static_cast<size_t>(n.texture * 0x9E3779B97F4A7C15ull ^ n.clut ^
                                       (static_cast<uint64_t>(n.bits) << 32 | static_cast<uint64_t>(n.regionW) << 16 | n.regionH));
        }
    };

    // A file's name without its extension, as PCSX2 reads it. False when it is not a replacement's name.
    bool parseName(const std::string &stem, Name &out);

    // The name of the texture `tex0` describes, `clamp` being CLAMP's register and `palette` the palette by texel index
    // (256 RGBA words, TEXA applied; only read for indexed formats). `vram` is the 4 MiB GS memory image.
    // `hasRegion` tells that CLAMP's region modes read only part of the texture (those textures are not replaced).
    // False for formats without a name (depth formats, anything unknown).
    bool nameOf(const uint8_t *vram, const GSTex0Reg &tex0, uint64_t clamp, const GSTexaReg &texa, const uint32_t *palette,
                Name &out, bool &hasRegion);

    // The `bits` part of the name: cheap, and what a pack's files can be filtered by before the texture is hashed.
    uint32_t nameBits(const GSTex0Reg &tex0, const GSTexaReg &texa);

    // The value of the environment variable `name` as UTF-8 (Windows environment variables are UTF-16), empty if unset.
    std::string environmentFolder(const char *name);

    // A decoded replacement: RGBA8 (R in the low byte, alpha as the GS has it: 0x80 is 1.0), every level one after the
    // other, largest first. The renderer makes the missing levels.
    struct Image
    {
        uint32_t width = 0, height = 0, levels = 1;
        std::vector<uint8_t> data;
        uint8_t minAlpha = 0, maxAlpha = 255;
    };

    // The images of a folder (searched recursively), read on background threads the first time a texture asks for them.
    class Pack
    {
    public:
        Pack();
        ~Pack();
        Pack(const Pack &) = delete;
        Pack &operator=(const Pack &) = delete;

        // Indexes `directory`; returns the number of replacements it holds (0 for none or no such folder).
        size_t open(const std::string &directory);
        size_t count() const;
        bool has(const Name &name) const;
        // Whether any file has these `bits` (valid once `open` returned): the test before a texture is hashed.
        bool wantsBits(uint32_t bits) const;

        // Hands over the image of `name` once it is read (the pack keeps no copy: the renderer keeps it on the GPU). While
        // it is read `pending` is set and null is returned; null with `pending` clear when the pack has no (readable) file.
        std::shared_ptr<const Image> take(const Name &name, bool &pending);

        // The decoded images waiting to be taken stay under this many bytes (the oldest are read again if drawn again).
        void setMemoryBudget(size_t bytes);

    private:
        struct Impl;
        std::unique_ptr<Impl> m_impl;
    };

    // Image decoders (also used by the tests): PNG / DDS file bytes to an RGBA8 image with its own levels. Null when the
    // bytes are not a supported file.
    std::shared_ptr<Image> decodePng(const uint8_t *bytes, size_t size);
    std::shared_ptr<Image> decodeDds(const uint8_t *bytes, size_t size);
}
