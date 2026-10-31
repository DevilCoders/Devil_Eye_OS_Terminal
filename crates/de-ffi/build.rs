fn main() {
    let sources = [
        "../../c/build_info.c",
        "../../c/abi.c",
        "../../c/shell.c",
        "../../c/internal/input_buffer.c",
        "../../c/internal/lexer.c",
        "../../c/internal/parser.c",
        "../../c/internal/command_registry.c",
        "../../c/execution.c",
    ];

    let mut build = cc::Build::new();
    build
        .files(sources)
        .include("../../include")
        .include("../../c")
        .warnings(true)
        .extra_warnings(true)
        .flag_if_supported("-std=c17")
        .flag_if_supported("-Wpedantic")
        .flag_if_supported("-Wconversion")
        .flag_if_supported("-Wsign-conversion")
        .flag_if_supported("-Wshadow")
        .flag_if_supported("-Wstrict-prototypes")
        .flag_if_supported("-Werror")
        .compile("de_shell");

    for source in sources {
        println!("cargo:rerun-if-changed={source}");
    }
    println!("cargo:rerun-if-changed=../../include/de_shell");
    println!("cargo:rerun-if-changed=../../c/internal");
}
