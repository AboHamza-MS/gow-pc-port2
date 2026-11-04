#pragma once

#include <cstdint>

namespace ps2x::iop::detail
{
    struct IopCpuState;
    class IopMemory;

    class IopSysclib
    {
    public:
        explicit IopSysclib(IopMemory &memory) noexcept;

        [[nodiscard]] bool dispatchImport(uint16_t ordinal, IopCpuState &cpu);

    private:
        IopMemory &m_memory;
        uint32_t m_strtokNext = 0u; // GOW-Port: where strtok(NULL, ...) continues; 0 when the string is finished
    };
}
