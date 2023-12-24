use editor::EditorApp;
use engine::eframe;
use reflect::ReflectDefault;
use std::any::TypeId;

fn main() -> eframe::Result<()> {
    println!("ReflectDefault - {:?}", TypeId::of::<ReflectDefault>());
    EditorApp::run()
}
