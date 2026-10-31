// GOW-Port: checks of the PCSX2-format texture replacement packs (gs_texture_replace); no game data, no OpenGL.
#include "runtime/gs/gs_swizzle.h"
#include "runtime/gs/gs_texture_replace.h"

#include <chrono>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <thread>
#include <vector>

using namespace gs_texrepl;

static int failures = 0;
#define CHECK(condition)                                                        \
    do                                                                          \
    {                                                                           \
        if (!(condition))                                                       \
        {                                                                       \
            std::fprintf(stderr, "FAILED line %d: %s\n", __LINE__, #condition); \
            ++failures;                                                         \
        }                                                                       \
    } while (0)

// 2x2 RGBA: (255,0,0,10) (0,255,0,20) / (0,0,255,30) (255,255,255,128)
static const uint8_t kPngRgba[] = {0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x06, 0x00, 0x00, 0x00, 0x72, 0xb6, 0x0d, 0x24, 0x00, 0x00, 0x00, 0x1a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xc0, 0xc5, 0xf0, 0x9f, 0x41, 0x84, 0x81, 0x81, 0xe1, 0xbf, 0xdc, 0xff, 0xff, 0xff, 0x1b, 0x00, 0x2e, 0x50, 0x06, 0xb7, 0x35, 0x48, 0x16, 0x23, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82};
// 2x1 RGB, no alpha: (1,2,3) (4,5,6)
static const uint8_t kPngRgb[] = {0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x7b, 0x40, 0xe8, 0xdd, 0x00, 0x00, 0x00, 0x0f, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x64, 0x62, 0x66, 0x61, 0x65, 0x03, 0x00, 0x00, 0x3f, 0x00, 0x16, 0x21, 0xba, 0xd4, 0x54, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82};

static void writeFile(const std::filesystem::path &path, const uint8_t *data, size_t size)
{
    std::ofstream file(path, std::ios::binary);
    file.write(reinterpret_cast<const char *>(data), static_cast<std::streamsize>(size));
}

static void testNames()
{
    // Names of real files of a PCSX2 pack for this game.
    Name n;
    CHECK(parseName("103ccbb0620a7eab-872500ea43ba11c3-00001e13", n));
    CHECK(n.texture == 0x103ccbb0620a7eabull && n.clut == 0x872500ea43ba11c3ull && n.bits == 0x1e13u && n.regionW == 0);
    CHECK(parseName("11f7ac75a003091e-b001353d99a65622-000015d3", n) && n.bits == 0x15d3u);
    // A hash without leading zeros (15 digits) and the way a name is spelt back.
    CHECK(parseName("1155fe6869e1a7d3-6251696217643d9-00001193", n) && n.clut == 0x6251696217643d9ull);
    CHECK(n.text() == "1155fe6869e1a7d3-6251696217643d9-00001193");
    // Without a CLUT, with a region, and TCC (bit 14) cleared as PCSX2 does.
    CHECK(parseName("abc-00004213", n) && n.clut == 0 && n.bits == 0x213u);
    CHECK(parseName("abc-def-r64x32-00000013", n) && n.regionW == 64 && n.regionH == 32 && n.clut == 0xdefu);
    CHECK(parseName("abc-r64x32-00000013", n) && n.clut == 0 && n.regionW == 64);
    CHECK(n.text() == "abc-r64x32-00000013");
    // Not names.
    CHECK(!parseName("readme", n));
    CHECK(!parseName("abc-123", n));      // bits need 8 digits
    CHECK(!parseName("xyz-00000013", n)); // not hex
    CHECK(!parseName("a-b-c-d-e-00000013", n));
}

static void testBits()
{
    GSTex0Reg t{};
    GSTexaReg texa{};
    t.psm = 0x13; // T8 256x256: the bits of 2213 in the pack
    t.tw = 8;
    t.th = 8;
    CHECK(nameBits(t, texa) == 0x2213u);
    t.psm = 0x14;
    t.tw = t.th = 6;
    CHECK(nameBits(t, texa) == (0x14u | 6u << 6 | 6u << 10));
    // 24- and 16-bit formats add TEXA.
    t.psm = 0x01;
    texa.ta0 = 0x80;
    texa.aem = true;
    texa.ta1 = 0x7F;
    CHECK(nameBits(t, texa) == (0x01u | 6u << 6 | 6u << 10 | 0x80u << 15 | 1u << 23 | 0x7Fu << 24));
    t.psm = 0x00; // CT32: none
    CHECK(nameBits(t, texa) == (0x00u | 6u << 6 | 6u << 10));
}

// A PSMT8 texture at block 0, width 2 (x64): pixel (x, y) = (x + y * 3) & 255.
static void fillT8(std::vector<uint8_t> &vram, uint32_t width, uint32_t height)
{
    const GSSwizzle::Format &f = GSSwizzle::GetFormat(0x13);
    for (uint32_t y = 0; y < height; ++y)
        for (uint32_t x = 0; x < width; ++x)
        {
            const GSSwizzle::Location at = GSSwizzle::Locate(f, 0, 2, x, y);
            GSSwizzle::StoreAt(f, vram.data() + at.byte, at.shift, (x + y * 3) & 255u);
        }
}

static void testHashing()
{
    std::vector<uint8_t> vram(GSSwizzle::kMemorySize, 0);
    fillT8(vram, 128, 128);
    GSTex0Reg t{};
    t.psm = 0x13;
    t.tbp0 = 0;
    t.tbw = 2;
    t.tw = 7;
    t.th = 7;
    GSTexaReg texa{};
    std::vector<uint32_t> palette(256);
    for (uint32_t i = 0; i < 256; ++i)
        palette[i] = 0x80000000u | i * 0x010101u;
    Name a, b;
    bool region = false;
    CHECK(nameOf(vram.data(), t, 0, texa, palette.data(), a, region) && !region);
    CHECK(a.bits == (0x13u | 7u << 6 | 7u << 10) && a.clut != 0 && a.texture != 0);
    // Deterministic, and sensitive to the texture's pixels and to the palette, nothing else.
    CHECK(nameOf(vram.data(), t, 0, texa, palette.data(), b, region) && a == b);
    const GSSwizzle::Format &f = GSSwizzle::GetFormat(0x13);
    const GSSwizzle::Location inside = GSSwizzle::Locate(f, 0, 2, 100, 100);
    vram[inside.byte] ^= 0x55;
    CHECK(nameOf(vram.data(), t, 0, texa, palette.data(), b, region) && b.texture != a.texture && b.clut == a.clut);
    vram[inside.byte] ^= 0x55;
    const GSSwizzle::Location outside = GSSwizzle::Locate(f, 0, 2, 0, 200); // another page
    vram[outside.byte] ^= 0x55;
    CHECK(nameOf(vram.data(), t, 0, texa, palette.data(), b, region) && b == a);
    palette[7] ^= 0x00FF00u;
    CHECK(nameOf(vram.data(), t, 0, texa, palette.data(), b, region) && b.texture == a.texture && b.clut != a.clut);
    // An indexed texture needs its palette; a depth format has no name.
    CHECK(!nameOf(vram.data(), t, 0, texa, nullptr, b, region));
    GSTex0Reg z = t;
    z.psm = 0x30;
    CHECK(!nameOf(vram.data(), z, 0, texa, palette.data(), b, region));
    // CLAMP region modes (REGION_CLAMP of 8 texels in a 128 wide texture) mean the texture is not replaced.
    const uint64_t clampRegion = 2ull | (0ull << 4) | (7ull << 14);
    CHECK(!nameOf(vram.data(), t, clampRegion, texa, palette.data(), b, region) && region);
    // A plain CLAMP / repeat is not a region.
    CHECK(nameOf(vram.data(), t, 1ull | (1ull << 2), texa, palette.data(), b, region) && !region);
}

static void testDecoders()
{
    auto rgba = decodePng(kPngRgba, sizeof kPngRgba);
    CHECK(rgba && rgba->width == 2 && rgba->height == 2 && rgba->levels == 1 && rgba->data.size() == 16);
    if (rgba)
    {
        CHECK(rgba->data[0] == 255 && rgba->data[1] == 0 && rgba->data[3] == 10); // R in the low byte, alpha as it is
        CHECK(rgba->minAlpha == 10 && rgba->maxAlpha == 128);
    }
    // A PNG without alpha gets 0x80, the GS's 1.0.
    auto rgb = decodePng(kPngRgb, sizeof kPngRgb);
    CHECK(rgb && rgb->width == 2 && rgb->height == 1 && rgb->data[3] == 0x80 && rgb->data[7] == 0x80 && rgb->data[4] == 4);
    CHECK(!decodePng(kPngRgb, 20));

    // DDS: a 4x4 DXT1 block of solid red, and a 2x2 uncompressed RGBA with two levels.
    std::vector<uint8_t> dds(128, 0);
    std::memcpy(dds.data(), "DDS ", 4);
    auto put32 = [&](size_t at, uint32_t v) { std::memcpy(dds.data() + at, &v, 4); };
    put32(4, 124);
    put32(8, 0x1007);
    put32(12, 4);          // height
    put32(16, 4);          // width
    put32(76, 32);         // pixel format size
    put32(80, 0x4);        // FOURCC
    put32(84, 0x31545844); // DXT1
    const uint8_t block[8] = {0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0}; // both endpoints 565 red, all indices 0
    dds.insert(dds.end(), block, block + 8);
    auto bc1 = decodeDds(dds.data(), dds.size());
    CHECK(bc1 && bc1->width == 4 && bc1->height == 4 && bc1->levels == 1);
    if (bc1)
        CHECK(bc1->data[0] > 240 && bc1->data[1] < 8 && bc1->data[2] < 8 && bc1->data[3] == 255);

    std::vector<uint8_t> raw(128, 0);
    std::memcpy(raw.data(), "DDS ", 4);
    auto set = [&](size_t at, uint32_t v) { std::memcpy(raw.data() + at, &v, 4); };
    set(4, 124);
    set(8, 0x2100F);
    set(12, 2);
    set(16, 2);
    set(28, 2);    // two mip levels
    set(76, 32);
    set(80, 0x41); // RGB | ALPHAPIXELS
    set(88, 32);   // bit count
    set(92, 0xFF); // masks: RGBA, R in the low byte
    const uint8_t texels[] = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20};
    raw.insert(raw.end(), texels, texels + sizeof texels);
    auto rawImage = decodeDds(raw.data(), raw.size());
    CHECK(rawImage && rawImage->levels == 2 && rawImage->data.size() == 20 && rawImage->data[0] == 1 && rawImage->data[16] == 17);
    CHECK(!decodeDds(raw.data(), 100));
}

static void testPack()
{
    namespace fs = std::filesystem;
    const fs::path dir = fs::temp_directory_path() / "gow_texrepl_test";
    fs::remove_all(dir);
    fs::create_directories(dir / "nested");
    writeFile(dir / "103ccbb0620a7eab-872500ea43ba11c3-00001e13.png", kPngRgba, sizeof kPngRgba);
    writeFile(dir / "nested" / "1068c9b87bfe5a78-1e62496bf628eb36-00001993.PNG", kPngRgb, sizeof kPngRgb);
    writeFile(dir / "notes.txt", kPngRgb, sizeof kPngRgb);
    writeFile(dir / "ab-00000013.png", kPngRgb, 10); // a name, but not a PNG
    writeFile(dir / "badext-00000013.bmp", kPngRgb, 10);

    Pack pack;
    CHECK(pack.open((dir / "missing").string()) == 0);
    CHECK(pack.open(dir.string()) == 3); // the two textures and the one with a name but a broken file
    Name first, second;
    CHECK(parseName("103ccbb0620a7eab-872500ea43ba11c3-00001e13", first));
    CHECK(parseName("1068c9b87bfe5a78-1e62496bf628eb36-00001993", second));
    CHECK(pack.has(first) && pack.has(second));
    CHECK(pack.wantsBits(0x1e13u) && pack.wantsBits(0x1993u) && !pack.wantsBits(0x1111u));

    bool pending = false;
    std::shared_ptr<const Image> image = pack.take(first, pending);
    CHECK(!image && pending); // the first ask starts the read
    for (int i = 0; i < 500 && !image; ++i)
    {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
        image = pack.take(first, pending);
    }
    CHECK(image && image->width == 2 && image->height == 2);
    // The pack hands an image over once: the next ask reads it again.
    bool again = false;
    CHECK(!pack.take(first, again) && again);
    // A file that cannot be decoded ends as "no file".
    Name broken;
    CHECK(parseName("ab-00000013", broken));
    std::shared_ptr<const Image> none;
    for (int i = 0; i < 500; ++i)
    {
        none = pack.take(broken, pending);
        if (!pending)
            break;
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    CHECK(!none && !pending);
    // A name the pack does not have.
    Name other = first;
    other.texture ^= 1;
    CHECK(!pack.take(other, pending) && !pending);
    fs::remove_all(dir);
}

int main()
{
    testNames();
    testBits();
    testHashing();
    testDecoders();
    testPack();
    if (failures)
    {
        std::fprintf(stderr, "%d check(s) failed\n", failures);
        return 1;
    }
    std::puts("texture replacement tests passed");
    return 0;
}
