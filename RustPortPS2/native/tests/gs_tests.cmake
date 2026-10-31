# Checks of the GS wrapper using the already patched runtime, without game data or OpenGL.
# Include from RustPortPS2/native/CMakeLists.txt after defining ps2_runtime.
add_executable(gow_gs_replay_test "${CMAKE_CURRENT_LIST_DIR}/gs_replay_test.cpp")
target_link_libraries(gow_gs_replay_test PRIVATE ps2_runtime)
add_executable(gow_gs_replay "${CMAKE_CURRENT_LIST_DIR}/../tools/render/replay_gs.cpp")
target_link_libraries(gow_gs_replay PRIVATE ps2_runtime)
add_executable(gow_gs_replay_checkpoints_test "${CMAKE_CURRENT_LIST_DIR}/gs_replay_checkpoints_test.cpp")
target_link_libraries(gow_gs_replay_checkpoints_test PRIVATE ps2_runtime)
add_executable(gow_texture_replace_test "${CMAKE_CURRENT_LIST_DIR}/texture_replace_test.cpp")
target_link_libraries(gow_texture_replace_test PRIVATE ps2_runtime)
add_executable(gow_gs_finish_async_test "${CMAKE_CURRENT_LIST_DIR}/gs_finish_async_test.cpp")
target_link_libraries(gow_gs_finish_async_test PRIVATE ps2_runtime)
