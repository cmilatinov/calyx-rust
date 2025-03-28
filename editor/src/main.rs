use editor::EditorApp;
use engine::eframe;
use engine::reflect::ReflectDefault;

fn main() -> eframe::Result<()> {
    println!(
        "Loading editor: {:?}",
        std::any::TypeId::of::<ReflectDefault>()
    );
    EditorApp::run()
}
