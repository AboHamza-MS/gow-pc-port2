// Standalone test of the memory card storage (ps2xIOP/src/emulator/core/memory_card.cpp): creation through a temporary
// file, the backup kept before the first write of a session, and a card of the wrong size moved aside instead of written
// over. It needs no game files and works in a temporary folder:
//
//   MSVC (x64 Native Tools prompt, from native/tests):
//     cl /nologo /std:c++20 /EHsc /W4 /I ..\ps2xIOP\src\emulator\core memory_card_test.cpp ..\ps2xIOP\src\emulator\core\memory_card.cpp
//   GCC / Clang:
//     c++ -std=c++20 -O2 -I ../ps2xIOP/src/emulator/core memory_card_test.cpp ../ps2xIOP/src/emulator/core/memory_card.cpp -o memory_card_test
//
// Not registered in CMake on purpose (a CMake edit recompiles the whole runtime); run it by hand after touching the card.
#include "memory_card.h"

#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <vector>

using ps2x::iop::detail::MemoryCard;
namespace fs = std::filesystem;

namespace
{
    int failures = 0;

    void check(bool condition, const char *what)
    {
        if (!condition)
        {
            std::printf("FAIL: %s\n", what);
            ++failures;
        }
    }

    std::vector<uint8_t> readFile(const fs::path &path)
    {
        std::ifstream file(path, std::ios::binary);
        return {std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()};
    }

    void writePage(MemoryCard &card, uint32_t page, uint8_t value)
    {
        std::vector<uint8_t> bytes(MemoryCard::RawPageSize, value);
        card.setAddress(page);
        card.write(bytes.data(), bytes.size());
    }
}

int main()
{
    const fs::path folder = fs::temp_directory_path() / "gow_memory_card_test";
    fs::remove_all(folder);
    fs::create_directories(folder);
    const std::string path = (folder / "card.ps2").string();

    {
        // A new card: created whole on the first write, with no temporary left and no backup (nothing older to keep).
        MemoryCard card;
        card.insert(path);
        writePage(card, 3, 0x00);
        writePage(card, 4, 0x0F);
        card.eject();
        const std::vector<uint8_t> image = readFile(path);
        check(image.size() == MemoryCard::ImageSize, "the new card has the full size");
        check(image.size() > 5 * MemoryCard::RawPageSize && image[3 * MemoryCard::RawPageSize] == 0x00 &&
                  image[4 * MemoryCard::RawPageSize] == 0x0F && image[0] == 0xFF,
              "the new card holds both pages");
        check(!fs::exists(path + ".tmp"), "no temporary is left");
        check(!fs::exists(path + ".bak"), "a new card has no backup");
    }
    {
        // A card that existed before the process: backed up before its first write.
        const std::string old = (folder / "old.ps2").string();
        fs::copy_file(path, old);
        const std::vector<uint8_t> before = readFile(old);
        MemoryCard card;
        card.insert(old);
        writePage(card, 7, 0x00);
        card.eject();
        check(readFile(old + ".bak") == before, "the backup is the card as it was");
        check(readFile(old)[7 * MemoryCard::RawPageSize] == 0x00, "the card itself is written");
        check(!fs::exists(old + ".bak.tmp"), "no temporary backup is left");
    }
    {
        // A cut card is moved aside, untouched, and the game gets a blank card in its place.
        const std::string cut = (folder / "cut.ps2").string();
        {
            std::ofstream file(cut, std::ios::binary);
            file << "a card cut by a crash";
        }
        MemoryCard card;
        card.insert(cut);
        check(card.image()[0] == 0xFF, "the game sees a blank card");
        check(readFile(cut + ".unusable") == std::vector<uint8_t>{'a', ' ', 'c', 'a', 'r', 'd', ' ', 'c', 'u', 't', ' ',
                                                                    'b', 'y', ' ', 'a', ' ', 'c', 'r', 'a', 's', 'h'},
              "the cut card is kept as .unusable");
        writePage(card, 0, 0x00);
        card.eject();
        check(readFile(cut).size() == MemoryCard::ImageSize, "a full card replaces it on the first write");
    }

    fs::remove_all(folder);
    if (failures == 0)
        std::printf("memory_card_test: ok\n");
    return failures == 0 ? 0 : 1;
}
