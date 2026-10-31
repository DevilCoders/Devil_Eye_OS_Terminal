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

# This is build metadata only. The DE-Shell lifecycle and language API are
# introduced separately by DE-004-01 after the cross-component contracts land.
add_library(de_shell STATIC
    c/build_info.c
)

target_include_directories(de_shell
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
)

target_compile_definitions(de_shell
    PRIVATE
        DE_SHELL_VERSION="${PROJECT_VERSION}"
)

de_enable_strict_warnings(de_shell)

if(BUILD_TESTING)
    add_executable(de_shell_build_info_test
        c/tests/test_build_info.c
    )

    target_link_libraries(de_shell_build_info_test PRIVATE de_shell)
    de_enable_strict_warnings(de_shell_build_info_test)

    add_test(
        NAME de_shell_build_info
        COMMAND de_shell_build_info_test
    )
endif()
