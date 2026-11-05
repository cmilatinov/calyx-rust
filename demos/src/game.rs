fn main() -> eframe::Result<()> {
    calyx_engine::runtime::run_player(env!("CARGO_MANIFEST_DIR"), demos::plugin_main)
}
