#[derive(Default)]
pub struct LauncherApp {
    search: String,
    boolean: bool
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let Self {
            search,
            boolean
        } = self;
        
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                ui.add(egui::TextEdit::singleline(search).hint_text("Search Here"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    if ui.button("New Project").clicked() {
                        *boolean = !*boolean;
                    }

                    if ui.button("Open").clicked() {
                        *boolean = !*boolean;
                    }
                });
            });
            ui.add_space(10.0);
            
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| {
                    for x in 0..50 {
                        ui.add_sized([120., 40.], egui::Button::new("My Button"));
                    }
                });
            });
        });
    }
}
