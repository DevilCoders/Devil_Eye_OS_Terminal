cmake_minimum_required(VERSION 3.25)

project(DevilEyeTerminal
    VERSION 0.1.0
    DESCRIPTION "Devil_Eye OS Terminal C subsystem"
    LANGUAGES C
)

include(CTest)

set(CMAKE_C_STANDARD 17)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_C_EXTENSIONS OFF)

option(DE_ENABLE_COVERAGE "Enable GCC/Clang source coverage instrumentation" OFF)

function(de_enable_strict_warnings target)
    if(CMAKE_C_COMPILER_ID MATCHES "GNU|Clang")
        target_compile_options(${target} PRIVATE
            -Wall
            -Wextra
            -Wpedantic
            -Wconversion
            -Wsign-conversion
            -Wshadow
            -Wstrict-prototypes
            -Werror
        )
    elseif(MSVC)
        target_compile_options(${target} PRIVATE /W4 /WX)
    endif()
endfunction()

# DE-Shell begins as an independent C library. Parser, command execution, and
# Rust/C ABI bindings are introduced by their dedicated tasks.
add_library(de_shell STATIC
    c/build_info.c
    c/shell.c
    c/internal/input_buffer.c
)

target_include_directories(de_shell
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/c
)

target_compile_definitions(de_shell
    PRIVATE
        DE_SHELL_VERSION="${PROJECT_VERSION}"
)

de_enable_strict_warnings(de_shell)

if(DE_ENABLE_COVERAGE)
    if(NOT CMAKE_C_COMPILER_ID MATCHES "GNU|Clang")
        message(FATAL_ERROR "DE_ENABLE_COVERAGE requires GCC or Clang")
    endif()

    target_compile_options(de_shell PRIVATE --coverage -O0 -g)
    # Propagate the coverage runtime link flag to executables consuming the
    # static library; static archives themselves do not perform a link step.
    target_link_options(de_shell INTERFACE --coverage)
endif()

if(BUILD_TESTING)
    add_executable(de_shell_build_info_test
        c/tests/test_build_info.c
    )

    target_link_libraries(de_shell_build_info_test PRIVATE de_shell)
    de_enable_strict_warnings(de_shell_build_info_test)

    if(DE_ENABLE_COVERAGE)
        target_compile_options(de_shell_build_info_test PRIVATE --coverage -O0 -g)
        target_link_options(de_shell_build_info_test PRIVATE --coverage)
    endif()

    add_test(
        NAME de_shell_build_info
        COMMAND de_shell_build_info_test
    )

    add_executable(de_shell_lifecycle_test
        c/tests/test_shell_lifecycle.c
    )

    target_link_libraries(de_shell_lifecycle_test PRIVATE de_shell)
    de_enable_strict_warnings(de_shell_lifecycle_test)

    if(DE_ENABLE_COVERAGE)
        target_compile_options(de_shell_lifecycle_test PRIVATE --coverage -O0 -g)
        target_link_options(de_shell_lifecycle_test PRIVATE --coverage)
    endif()

    add_test(
        NAME de_shell_lifecycle
        COMMAND de_shell_lifecycle_test
    )

    add_executable(de_shell_input_buffer_test
        c/tests/test_input_buffer.c
    )

    target_include_directories(de_shell_input_buffer_test
        PRIVATE
            ${CMAKE_CURRENT_SOURCE_DIR}/c
    )
    target_link_libraries(de_shell_input_buffer_test PRIVATE de_shell)
    de_enable_strict_warnings(de_shell_input_buffer_test)

    if(DE_ENABLE_COVERAGE)
        target_compile_options(de_shell_input_buffer_test PRIVATE --coverage -O0 -g)
        target_link_options(de_shell_input_buffer_test PRIVATE --coverage)
    endif()

    add_test(
        NAME de_shell_input_buffer
        COMMAND de_shell_input_buffer_test
    )
endif()
