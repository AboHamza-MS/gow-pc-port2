include(CheckIPOSupported)

check_ipo_supported(RESULT IPO_SUPPORTED OUTPUT IPO_ERROR)

# GOW-Port: GCC/Clang link with LTO in as many processes as cores (-flto=auto), and with ~6,400 files each process
# uses 1-2 GB: on an 8-16 GB machine it ran out of memory. PS2X_LTO_JOBS=N limits it (the installers choose it
# according to memory); empty = CMake's behavior.
set(PS2X_LTO_JOBS "" CACHE STRING "GCC/Clang: LTO link processes (empty = all cores)")

# GOW-Port: /GL + /LTCG over ~6,400 files left MSVC's linker more than 8 hours without producing the EXE
# (docs/dev/BUILD.md), so the normal Release does not use them: it builds and links in minutes. /fp:fast and /arch:AVX2
# have not been validated with the game either (the tested variant is RelWithDebInfo, with CMake's default options).
option(PS2X_ENABLE_LTCG "MSVC: whole-program optimization (/GL /LTCG); linking can take hours" OFF)
option(PS2X_FAST_MATH_AVX2 "MSVC: /fp:fast and /arch:AVX2 (not validated with the game)" OFF)

function(EnableFastReleaseMode TargetName)
    message("> Enabling optimization for: ${TargetName}")
    if(MSVC)
        target_compile_options(${TargetName} PRIVATE
            $<$<CONFIG:Release>:
                /O2 # speed
                /Ob2 # inline aggressively
                /Oi # intrinsics
                /Gy # function-level linking
                /Gw # global data in COMDAT
                /GF # string pooling
                /Zc:inline # remove unreferenced inline
                /DNDEBUG
                /GS- # Disable Buffer Security Check (faster)
                /Qspectre- # Disable Spectre mitigations (faster)
            >
        )
        if(PS2X_ENABLE_LTCG)
            target_compile_options(${TargetName} PRIVATE $<$<CONFIG:Release>:/GL>)
        endif()
        if(PS2X_FAST_MATH_AVX2)
            target_compile_options(${TargetName} PRIVATE $<$<CONFIG:Release>:/fp:fast /arch:AVX2>)
        endif()

        if(TARGET ${TargetName})
            target_link_options(${TargetName} PRIVATE
                $<$<CONFIG:Release>:
                    /OPT:REF # remove unreferenced
                    /OPT:ICF # fold identical COMDATs
                >
            )
            if(PS2X_ENABLE_LTCG)
                target_link_options(${TargetName} PRIVATE $<$<CONFIG:Release>:/LTCG>)
            endif()
        endif()
    endif()

    # On MSVC cross-unit optimization (IPO) is /GL + /LTCG: only with PS2X_ENABLE_LTCG. GOW-Port: on Windows with clang
    # (llvm-mingw) too: LTO over ~6,400 files multiplies the link time and memory and was never measured faster (gow-native-pc
    # keeps its ThinLTO opt-in for that reason). Linux with GCC keeps its LTO (with PS2X_LTO_JOBS); clang on Linux matches
    # Windows clang (LTO opt-in with PS2X_ENABLE_LTCG, and no GCC-only -flto=<N> job count flag).
    if(IPO_SUPPORTED AND ((CMAKE_CXX_COMPILER_ID STREQUAL "GNU" AND NOT WIN32) OR PS2X_ENABLE_LTCG))
        set_property(TARGET ${TargetName} PROPERTY INTERPROCEDURAL_OPTIMIZATION_RELEASE TRUE)
        if(CMAKE_CXX_COMPILER_ID STREQUAL "GNU" AND PS2X_LTO_JOBS)
            target_link_options(${TargetName} PRIVATE $<$<CONFIG:Release>:-flto=${PS2X_LTO_JOBS}>)
        endif()
    elseif(NOT IPO_SUPPORTED)
        message(WARNING "Interprocedural optimization not supported: ${IPO_ERROR}")
    else()
        message(STATUS "Interprocedural optimization off for ${TargetName} (PS2X_ENABLE_LTCG=ON enables it)")
    endif()
endfunction()