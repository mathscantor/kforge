use argparse::{ArgumentParser, StoreTrue, Store};
use binwalk::Binwalk;
use tabled::{
    Tabled, Table
};
use tabled::settings::{
    style::Style
};
use std::str;
use std::process;
use std::env;
use std::fs;
use std::path::Path; 

extern crate pretty_env_logger;
#[macro_use] extern crate log;

const KFORGE_VERSION: &'static str = "v1.0.0";

fn get_version() -> &'static str {
    return KFORGE_VERSION;
}

fn set_log_level(level: &str) {
    unsafe {
        env::set_var("RUST_LOG", level);
    }
    assert_eq!(env::var("RUST_LOG"), Ok(level.to_string()));
    return;
}

fn main() {

    let mut verbose: bool = false;
    let mut version: bool = false;
    let mut filepath: String = "".to_string();
    let description = format!("KForge {} takes in a compressed kernel and returns a step-by-step process of kernel patching without the source code. \
                                      It is especially useful when customizing firmware for embedded devices.", get_version());

    {  // this block limits scope of borrows by ap.refer() method
        let mut ap = ArgumentParser::new();

        ap.set_description(&description);
        ap.refer(&mut verbose).add_option(
            &["-v", "--verbose"], 
            StoreTrue,
            "Show debug logs");
        ap.refer(&mut filepath).add_option(
            &["-f", "--file"], 
            Store,
            "The filepath of the compressed kernel (Eg. vmlinuz)"
        );
        ap.refer(&mut version).add_option(
            &["-V", "--version"], 
            StoreTrue,
            "Prints the version of KForge"
        );
        ap.parse_args_or_exit();
    }

    set_log_level("info"); // Default logging level
    if verbose {
        set_log_level("debug");
    }
    pretty_env_logger::init();

    if version {
        info!("KForge {}", get_version());
        process::exit(0);
    }

    if filepath.is_empty() {
        error!("Missing required [-f | --file] argument!");
        process::exit(1);
    }
    
    let path = Path::new(&filepath);

    if !path.exists() {
        error!("File '{}' does not exist.", path.display());
        process::exit(1);
    }
    if !path.is_file() {
        error!("'{}' is not a regular file.", path.display());
        process::exit(1);
    }

    let parent_str: Option<&str> = path.parent().and_then(|p| p.to_str());
    let filename_str: Option<&str> = path.file_name().and_then(|f| f.to_str());
    match (parent_str, filename_str) {
        (Some(parent), Some(filename)) => {
            info!("Analyzing '{}/{}'...", parent, filename);
            analyze(parent, filename)
        }
        _ => {
            error!("Failed to extract parent directory or file name");
        }
    }

}

fn analyze(parent_dir: &str, filename: &str) {

    // Create a new Binwalk instance
    let binwalker = Binwalk::new();

    // Read in the data you want to analyze
    let file_data = fs::read(format!("{}/{}", parent_dir, filename)).expect("Failed to read from file");

    // Scan the file data and print the results
    let scan_results = binwalker.scan(&file_data);
    if scan_results.len() == 0 {
        error!("Unable to find any compressed sections in '{filename}'");
        process::exit(1);
    } else if scan_results.len() > 1 {
        warn!("Identified more than one compressed section! Only one of them contains the actual kernel. Please identify it yourself.");
    }

    for result in &scan_results {
        let (compressed_section_suffix, decompression_command, compression_command) = match result.name.as_str() {
            "zstd" => (".zst", "zstd -k -d", "zstd -k -19"),
            "xz" => (".xz", "xz -k -d", "xz -k -9"),
            "gzip" => (".gz", "gzip -k -d", "gzip -k -9"),
            "bzip2" => (".bz2", "bzip2 -k -d", "bzip2 -k -9"),
            "lz4" => (".lz4", "lz4 -k -d", "lz4 -k -9"),
            "lzop" => (".lzo", "lzop -k -d", "lzop -k -9"),
            "lzma" => (".lzma", "lzma -k -d", "lzma -k -9"),
            "lzfse" => (".lzfse", "lzfse -decode -o vmlinux -i", "lzfse -encode -o vmlinux.lzfse -i"),
            _ => {
                error!("Unable to find valid compression! Got '{}' instead.", result.name);
                process::exit(1);
            }
        };
        print_blueprint(parent_dir, filename, compressed_section_suffix, result.offset, result.size, decompression_command, compression_command, result.name.as_str());
    }
    return;
}

fn print_blueprint(parent_dir: &str, 
                   filename: &str, 
                   compressed_section_suffix: &str, 
                   compression_offset: usize, 
                   compression_size: usize,
                   decompression_command: &str,
                   compression_command: &str,
                   compression_type: &str) {
    /* Generate instructions for users */

    #[derive(Tabled)]
    struct Blueprint {
        step: usize,
        description: &'static str,
        commands: String,
    }

    let blueprint = vec![
        Blueprint{ step: 1, description: "Extraction", commands: format!("$ cd {parent_dir}\n\
                                                                          $ dd if={filename} of=vmlinux{compressed_section_suffix} ibs=1 skip=$[{compression_offset:#x}] count=$[{compression_size:#x}]") },
        Blueprint{ step: 2, description: "Decompression", commands: format!("$ {decompression_command} vmlinux{compressed_section_suffix}") },
        Blueprint{ step: 3, description: "Backup Orignal Files", commands: format!("$ cp vmlinux vmlinux.orig\n\
                                                                                    $ cp {filename} {filename}.orig") },
        Blueprint{ step: 4, description: "DIY Vmlinux Patching", commands: String::from("# ----------------- Patch out integrity checks ----------------- #\n\
                                                                                        └── Tip 1: Look for memcmp() calls in proximity of machine_halt(),\n\
                                                                                        kernel_restart() etc.\n\
                                                                                        └── Tip 2: Look for integrity related strings and patch these\n\
                                                                                        functions. Eg. \"firmware integrity\"") },
        Blueprint{ step: 5, description: "Recompression", commands: format!("$ {compression_command} vmlinux") },
        Blueprint{ step: 6, description: "Size Verification", commands: format!("$ wc -c vmlinux{compressed_section_suffix} | awk '{{printf \"0x%x\\n\", $1}}'\n\n\
                                                                                 # ----------------- Do This Before Proceeding ----------------- #\n\
                                                                                 └── Output must be <= {compression_size:#x}\n\
                                                                                 └── Else, recompress again with more aggressive parameters.") },
        Blueprint{ step: 7, description: "Zero Out Original Section", commands: format!("$ dd if=/dev/zero of={filename} bs=1 seek=$[{compression_offset:#x}] count=$[{compression_size:#x}] conv=notrunc") },
        Blueprint{ step: 8, description: "Overwrite Original Section", commands: format!("$ dd if=vmlinux{compressed_section_suffix} of={filename} bs=1 seek=$[{compression_offset:#x}] conv=notrunc") },
    ];

    let mut table = Table::new(blueprint);
    table.with(Style::modern());

    info!("Kernel Modification Blueprint (Type: {compression_type}, Offset: {compression_offset}, Size: {compression_size} bytes)\n{table}");
}