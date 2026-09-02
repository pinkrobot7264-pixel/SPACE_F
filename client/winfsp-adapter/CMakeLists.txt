# Phase 0 builds an empty adapter library plus a tiny probe executable. The
# library proves the WinFsp headers are found; the executable proves the import
# library winfsp-x64.lib actually links. The real callback table is Phase 1.

add_library(space_winfsp_adapter STATIC adapter.cpp)
target_link_libraries(space_winfsp_adapter PRIVATE WinFsp::WinFsp)

add_executable(space_winfsp_probe probe.cpp)
target_link_libraries(space_winfsp_probe PRIVATE space_winfsp_adapter WinFsp::WinFsp)
