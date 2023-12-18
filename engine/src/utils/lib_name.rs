pub fn lib_file_name(lib: &str) -> String {
    #[cfg(windows)]
    return format!("{lib}.dll");
    #[cfg(unix)]
    return format!("lib{lib}.so");
}
