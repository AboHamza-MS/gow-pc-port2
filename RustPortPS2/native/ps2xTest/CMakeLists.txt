cmake_minimum_required(VERSION 3.21)

project(ps2xTest LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

include(CTest)

# Static library with test logic (no main)
add_library(ps2_test_function_table OBJECT
    src/test_function_table.cpp
)

target_include_directories(ps2_test_function_table PRIVATE
    ${CMAKE_SOURCE_DIR}/ps2xRuntime/include
    ${CMAKE_SOURCE_DIR}/ps2xIOP/include
)

add_library(ps2_test_lib STATIC
    src/pad_input_tests.cpp
    src/ps2_runtime_io_tests.cpp
    src/ps2_runtime_kernel_tests.cpp
    src/ps2_runtime_interrupt_tests.cpp
    src/ps2_memory_tests.cpp
    src/ps2_vu1_tests.cpp
    src/ps2_vu_tests.cpp
    src/ps2_gs_tests.cpp
    src/ps2_gpu_profile_tests.cpp # GOW-Port: windows and timestamp reads without waiting.
    src/ps2_gpu_upload_ring_tests.cpp # GOW-Port: pending data and upload fallback.
    src/ps2_gs_shared_queue_tests.cpp # GOW-Port: FIFO and on-demand captures.
    src/ps2_iop_tests.cpp
    src/ps2_sif_rpc_tests.cpp
    src/ps2_sif_dma_tests.cpp
    src/ps2_runtime_expansion_tests.cpp
    src/gow_render_server_tests.cpp # GOW-Port: guards the render-server dispatch lifecycle.
    ${CMAKE_SOURCE_DIR}/ps2xRuntime/src/game/gow_render_server.cpp # GOW-Port: test the production override directly.
)

target_include_directories(ps2_test_lib PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_SOURCE_DIR}/ps2xRuntime/include
    ${CMAKE_SOURCE_DIR}/ps2xRuntime/src/lib
    ${CMAKE_SOURCE_DIR}/ps2xRuntime/src/game # GOW-Port: override headers shared by the test and production code.
)

target_link_libraries(ps2_test_lib PRIVATE
    ps2_runtime
    ps2_iop
)

if(UNIX AND NOT APPLE)
    target_link_libraries(ps2_test_lib PRIVATE ${CMAKE_DL_LIBS})
endif()

add_executable(ps2x_tests
    src/main.cpp
    $<TARGET_OBJECTS:ps2_test_function_table>
)

option(PRINT_GENERATED_CODE "Print generated code in tests" OFF)
if(PRINT_GENERATED_CODE)

    target_compile_definitions(ps2x_tests PRIVATE PRINT_GENERATED_CODE)
    target_compile_definitions(ps2_test_lib PRIVATE PRINT_GENERATED_CODE)
endif()

target_include_directories(ps2x_tests PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_SOURCE_DIR}/ps2xRuntime/include
)

target_link_libraries(ps2x_tests PRIVATE
    ps2_test_lib
    ps2_runtime
)

# VU/GS tests intentionally instantiate large interpreter state.
# Windows executables default to a much smaller stack than Linux, and
# sanitizer/debug instrumentation can push these test frames over 1 MiB.
if(MSVC)
    target_link_options(ps2x_tests PRIVATE "/STACK:8388608")
elseif(MINGW)
    # GOW-Port: through the compiler driver (clang / gcc), so the linker option needs the -Wl, prefix.
    target_link_options(ps2x_tests PRIVATE "-Wl,--stack,8388608")
endif()

if(COMMAND ps2x_stage_ffmpeg_runtime_dlls)
    ps2x_stage_ffmpeg_runtime_dlls(ps2x_tests)
endif()

include("${CMAKE_SOURCE_DIR}/ps2xRuntime/cmake/ReleaseMode.cmake")

if(CMAKE_BUILD_TYPE STREQUAL "Release" OR CMAKE_BUILD_TYPE STREQUAL "RelWithDebInfo")
    EnableFastReleaseMode(ps2_test_lib)
    EnableFastReleaseMode(ps2x_tests)
endif()
