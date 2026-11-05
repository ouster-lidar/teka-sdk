fn main() {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let plugin = std::env::args_os()
        .next()
        .and_then(|path| {
            std::path::PathBuf::from(path)
                .file_name()
                .map(|name| name.to_owned())
        })
        .is_some_and(|name| name == "teka-build-vm" || name == "teka build vm");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if plugin {
        std::process::exit(teka_cli::build_vm::main_entry());
    }
    std::process::exit(teka_cli::main_entry());
}
