use fs_extra::dir::CopyOptions;

fn main() {
    let _ = fs_extra::copy_items(&["../assets"], "./", &CopyOptions::new());
}
