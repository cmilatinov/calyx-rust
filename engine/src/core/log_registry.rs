// use log::{Log, Metadata, Record};
//
// #[derive(Default)]
// pub struct LogRegistry {
//     logs: Vec<String>,
// }
//
// impl LogRegistry {
//     pub fn drain_logs(&mut self) -> Vec<String> {
//         self.logs.drain(..).collect()
//     }
// }
//
// pub struct Logger;
//
// impl Log for Logger {
//     fn enabled(&self, metadata: &Metadata) -> bool {
//         if metadata.target().starts_with("wgpu")
//             || metadata.target().starts_with("eframe")
//             || metadata.target().starts_with("naga")
//             || metadata.target().starts_with("egui")
//             || metadata.target().starts_with("winit")
//         {
//             return false;
//         }
//         true
//     }
//     fn log(&self, record: &Record) {
//         if self.enabled(record.metadata()) {
//             let log_msg = format!("{}: {}", record.level(), record.args());
//             let mut registry = LogRegistry::get_mut();
//             registry.logs.push(log_msg);
//         }
//     }
//
//     fn flush(&self) {}
// }
//
// singleton_with_init!(LogRegistry);
