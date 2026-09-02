# Locate WinFsp via its registry entry, with a sane fallback.
cmake_host_system_information(RESULT _winfsp_dir
    QUERY WINDOWS_REGISTRY "HKLM/SOFTWARE/WOW6432Node/WinFsp"
    VALUE "InstallDir"
    ERROR_VARIABLE _winfsp_reg_err)

if(NOT _winfsp_dir)
    set(_winfsp_dir "C:/Program Files (x86)/WinFsp")
endif()

set(WINFSP_INSTALL_DIR "${_winfsp_dir}" CACHE PATH "WinFsp installation directory")

if(NOT EXISTS "${WINFSP_INSTALL_DIR}/inc/winfsp/winfsp.h")
    message(FATAL_ERROR
        "WinFsp developer files not found under '${WINFSP_INSTALL_DIR}'.\n"
        "Re-run the WinFsp MSI and SELECT THE 'Developer' FEATURE.")
endif()

add_library(WinFsp::WinFsp INTERFACE IMPORTED)

target_include_directories(WinFsp::WinFsp INTERFACE
    "${WINFSP_INSTALL_DIR}/inc")

target_link_libraries(WinFsp::WinFsp INTERFACE
    "${WINFSP_INSTALL_DIR}/lib/winfsp-x64.lib")

set(WinFsp_FOUND TRUE)

message(STATUS "WinFsp found: ${WINFSP_INSTALL_DIR}")