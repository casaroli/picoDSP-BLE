//! This build script copies the `memory.x` file from the crate root into
//! a directory where the linker can always find it at build time.
//! For many projects this is optional, as the linker always searches the
//! project root directory -- wherever `Cargo.toml` is. However, if you
//! are using a workspace or have a more complicated build setup, this
//! build script becomes required. Additionally, by requesting that
//! Cargo re-run the build script whenever `memory.x` is changed,
//! updating `memory.x` ensures a rebuild of the application with the
//! new memory settings.

use std::env;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    // Put `memory.x` in our output directory and ensure it's
    // on the linker search path.
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("memory.x"))
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());

    // By default, Cargo will re-run a build script whenever
    // any file in the project changes. By specifying `memory.x`
    // here, we ensure the build script is only re-run when
    // `memory.x` is changed.
    println!("cargo:rerun-if-changed=memory.x");

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");

    // --- RAM & Flash Calculation ---
    println!("cargo:rerun-if-changed=memory.x");

    let memory_x_content = fs::read_to_string("memory.x").unwrap();
    let mut total_ram_kb = 0;
    let mut total_flash_kb = 0;

    for line in memory_x_content.lines() {
        let line = line.trim();

        // RAM Calculation
        if (line.starts_with("RAM") || line.starts_with("SRAM")) && line.contains("LENGTH =") {
            if let Some(len_part) = line.split("LENGTH =").nth(1) {
                let len_str = len_part
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('K');
                if let Ok(kb) = len_str.parse::<u32>() {
                    total_ram_kb += kb;
                }
            }
        }

        // Flash Calculation
        if line.starts_with("FLASH") && line.contains("LENGTH =") {
            if let Some(len_part) = line.split("LENGTH =").nth(1) {
                let len_str = len_part
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('K');
                if let Ok(kb) = len_str.parse::<u32>() {
                    total_flash_kb += kb;
                }
            }
        }
    }

    println!("cargo:rustc-env=TOTAL_RAM_KB={}", total_ram_kb);
    println!("cargo:rustc-env=TOTAL_FLASH_KB={}", total_flash_kb);

    // --- Get infinitedsp-core version ---
    let version =
        get_dependency_version("infinitedsp-core").unwrap_or_else(|| "Unknown".to_string());
    println!("cargo:rustc-env=INFINITEDSP_CORE_VERSION={}", version);
}

fn get_dependency_version(pkg_name: &str) -> Option<String> {
    let lock_content = fs::read_to_string("Cargo.lock").ok()?;
    let mut in_package = false;
    let mut current_name = String::new();

    for line in lock_content.lines() {
        if line.trim() == "[[package]]" {
            in_package = true;
            current_name.clear();
            continue;
        }

        if in_package {
            let parts: Vec<&str> = line.split('=').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                if parts[0] == "name" {
                    current_name = parts[1].trim_matches('"').to_string();
                } else if parts[0] == "version" && current_name == pkg_name {
                    return Some(parts[1].trim_matches('"').to_string());
                }
            }
        }
    }
    None
}
