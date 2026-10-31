// GOW-Port: registry used when no IRX module has been compiled (ps2-tools iop-gen writes iopn_registry.cpp).
#include "native/iop_native.h"

namespace ps2x::iop::detail
{
    const IopNativeModule *iopNativeModules(size_t &count)
    {
        count = 0;
        return nullptr;
    }
}
