cmake_minimum_required(VERSION 3.21)

project(ps2xIOP LANGUAGES CXX)

option(PS2X_IOP_BUILD_TESTS "Build ps2xIOP emulator smoke tests" OFF)

add_library(ps2_iop STATIC
    src/ps2_path.cpp
    src/disc_source.cpp
    src/iop_module_manager.cpp
    src/iop_subsystem.cpp
    src/emulator/iop_emulator.cpp
    src/emulator/core/iop_cpu.cpp
    src/emulator/core/iop_kernel.cpp
    src/emulator/core/iop_memory.cpp
    src/emulator/core/sound_engine.cpp
    src/emulator/core/memory_card.cpp
    src/emulator/services/iop_module_loader.cpp
    src/emulator/services/iop_rpc.cpp
    src/emulator/imports/iop_cdvd.cpp
    src/emulator/imports/iop_heaplib.cpp
    src/emulator/imports/iop_imports.cpp
    src/emulator/imports/iop_intrman.cpp
    src/emulator/imports/iop_ioman.cpp
    src/emulator/imports/iop_loadcore.cpp
    src/emulator/imports/iop_stdio.cpp
    src/emulator/imports/iop_sysclib.cpp
    src/emulator/imports/iop_sysmem.cpp
    src/emulator/imports/iop_timrman.cpp
    src/emulator/imports/iop_vblank.cpp
    src/modules/dbcman.cpp
    src/modules/libsd.cpp
    src/modules/mcserv.cpp
    src/modules/gow_stub_services.cpp
)

# GOW-Port: IRX modules compiled ahead of time (ps2-tools iop-gen)
# (generated from the game's modules, not in git; without them the empty registry is used and the IRX code is interpreted)
file(GLOB PS2X_IOP_NATIVE_SOURCES CONFIGURE_DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/src/native/iopn_*.cpp)
if(NOT EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/src/native/iopn_registry.cpp)
    list(APPEND PS2X_IOP_NATIVE_SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/src/native/iop_native_none.cpp)
endif()
# GOW-Port: each part file holds a few huge switch-and-goto functions (~1.5 MB of C++), and the compiler needs several GB for
# one of them. Ten at once (one per build job) ran MSVC out of memory (C1002), so they build in their own Ninja pool, sized
# from the RAM (~6 GB per compile; PS2X_IOP_NATIVE_JOBS overrides it). The rest of the build keeps all the jobs.
cmake_host_system_information(RESULT PS2X_HOST_MEMORY_MB QUERY TOTAL_PHYSICAL_MEMORY)
math(EXPR PS2X_IOP_NATIVE_JOBS_DEFAULT "${PS2X_HOST_MEMORY_MB} / 6144")
if(PS2X_IOP_NATIVE_JOBS_DEFAULT LESS 1)
    set(PS2X_IOP_NATIVE_JOBS_DEFAULT 1)
endif()
set(PS2X_IOP_NATIVE_JOBS "${PS2X_IOP_NATIVE_JOBS_DEFAULT}" CACHE STRING "Native IOP files compiled at the same time (Ninja)")
set_property(GLOBAL APPEND PROPERTY JOB_POOLS ps2x_iop_native=${PS2X_IOP_NATIVE_JOBS})
add_library(ps2_iop_native OBJECT ${PS2X_IOP_NATIVE_SOURCES})
target_compile_features(ps2_iop_native PRIVATE cxx_std_20)
target_include_directories(ps2_iop_native PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/include ${CMAKE_CURRENT_SOURCE_DIR}/src)
set_property(TARGET ps2_iop_native PROPERTY JOB_POOL_COMPILE ps2x_iop_native)
if(MSVC)
    target_compile_options(ps2_iop_native PRIVATE /bigobj)
endif()
target_sources(ps2_iop PRIVATE $<TARGET_OBJECTS:ps2_iop_native> ${CMAKE_CURRENT_SOURCE_DIR}/src/native/iop_native_overrides.cpp)

target_compile_features(ps2_iop PUBLIC cxx_std_20)

target_include_directories(ps2_iop
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:include>
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/src
)

add_library(ps2x::iop ALIAS ps2_iop)

if(PS2X_IOP_BUILD_TESTS)
    enable_testing()
    add_executable(ps2_iop_emulator_tests tests/iop_emulator_tests.cpp)
    target_link_libraries(ps2_iop_emulator_tests PRIVATE ps2_iop)
    add_test(NAME ps2_iop_emulator_tests COMMAND ps2_iop_emulator_tests)

    add_executable(ps2_iop_import_tests tests/iop_import_tests.cpp)
    target_link_libraries(ps2_iop_import_tests PRIVATE ps2_iop)
    target_include_directories(ps2_iop_import_tests PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/src)
    add_test(NAME ps2_iop_import_tests COMMAND ps2_iop_import_tests)

    add_executable(ps2_iop_compatibility_tests tests/iop_compatibility_tests.cpp)
    target_link_libraries(ps2_iop_compatibility_tests PRIVATE ps2_iop)
    add_test(NAME ps2_iop_compatibility_tests COMMAND ps2_iop_compatibility_tests)

    add_executable(ps2_iop_import_version_tests tests/iop_import_version_tests.cpp)
    target_link_libraries(ps2_iop_import_version_tests PRIVATE ps2_iop)
    target_include_directories(ps2_iop_import_version_tests PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/src)
    add_test(NAME ps2_iop_import_version_tests COMMAND ps2_iop_import_version_tests)

endif()

install(TARGETS ps2_iop
    ARCHIVE DESTINATION lib
    LIBRARY DESTINATION lib
    RUNTIME DESTINATION bin
)

install(DIRECTORY include/
    DESTINATION include
)
