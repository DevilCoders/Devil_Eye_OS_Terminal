use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SOURCES: &[&str] = &[
    "c/build_info.c",
    "c/abi.c",
    "c/shell.c",
    "c/internal/input_buffer.c",
    "c/internal/lexer.c",
    "c/internal/parser.c",
    "c/internal/command_registry.c",
    "c/execution.c",
];

fn run(command: &mut Command, description: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("failed to start {description}: {error}"));
    if !status.success() {
        panic!("{description} failed with status {status}");
    }
}

fn command_parts(value: Option<String>, fallback: &str) -> (String, Vec<String>) {
    let value = value.unwrap_or_else(|| fallback.to_owned());
    let mut parts = value.split_whitespace();
    let executable = parts
        .next()
        .unwrap_or_else(|| panic!("empty compiler command"))
        .to_owned();
    (executable, parts.map(str::to_owned).collect())
}

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_os != "linux" || target_arch != "x86_64" {
        panic!("DE-Shell FFI native build currently supports Linux x86_64 only");
    }

    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"),
    );
    let repository_root = manifest_dir.join("../..");
    let include_dir = repository_root.join("include");
    let c_dir = repository_root.join("c");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));

    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    println!("cargo:rerun-if-env-changed=CFLAGS");
    println!("cargo:rerun-if-env-changed=OPT_LEVEL");
    println!("cargo:rerun-if-env-changed=DEBUG");

    let (compiler, compiler_args) = command_parts(env::var("CC").ok(), "cc");
    let extra_flags = env::var("CFLAGS").unwrap_or_default();
    let optimization = env::var("OPT_LEVEL").unwrap_or_else(|_| "0".to_owned());
    let debug = env::var("DEBUG").unwrap_or_default() == "true";

    let mut objects = Vec::with_capacity(SOURCES.len());
    for (index, source) in SOURCES.iter().enumerate() {
        let source_path = repository_root.join(source);
        let object_path = out_dir.join(format!("de_shell_{index}.o"));
        let mut command = Command::new(&compiler);
        command
            .args(&compiler_args)
            .arg("-std=c17")
            .arg("-fPIC")
            .arg("-Wall")
            .arg("-Wextra")
            .arg("-Wpedantic")
            .arg("-Wconversion")
            .arg("-Wsign-conversion")
            .arg("-Wshadow")
            .arg("-Wstrict-prototypes")
            .arg("-Werror")
            .arg(format!("-O{optimization}"))
            .arg(format!("-I{}", include_dir.display()))
            .arg(format!("-I{}", c_dir.display()));
        if debug {
            command.arg("-g");
        }
        command.args(extra_flags.split_whitespace());
        command
            .arg("-c")
            .arg(&source_path)
            .arg("-o")
            .arg(&object_path);
        run(&mut command, &format!("C compilation of {}", source_path.display()));

        println!("cargo:rerun-if-changed={}", source_path.display());
        objects.push(object_path);
    }

    for directory in [include_dir.join("de_shell"), c_dir.join("internal")] {
        println!("cargo:rerun-if-changed={}", directory.display());
    }

    let archive = out_dir.join("libde_shell.a");
    let _ = fs::remove_file(&archive);
    let (archiver, archiver_args) = command_parts(env::var("AR").ok(), "ar");
    let mut command = Command::new(archiver);
    command.args(archiver_args).arg("crus").arg(&archive).args(&objects);
    run(&mut command, "DE-Shell static archive creation");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=de_shell");

}
