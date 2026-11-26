use super::*;

#[test]
fn buttons_fit_the_view_and_do_not_overlap() {
    for (width, height) in [
        (1280.0, 720.0),
        (960.0, 540.0),
        (2560.0, 1440.0),
        (640.0, 480.0),
    ] {
        for screen in [
            MenuScreen::MainMenu,
            MenuScreen::Lobby,
            MenuScreen::GameOver,
        ] {
            let buttons = menu_buttons(screen, width, height);
            assert_eq!(buttons.len(), 2);
            assert_eq!(buttons.iter().filter(|b| b.primary).count(), 1);
            for b in &buttons {
                assert!(b.x >= 0.0 && b.y >= 0.0, "{screen:?} {b:?}");
                assert!(
                    b.x + b.w <= width && b.y + b.h <= height,
                    "{screen:?} {b:?}"
                );
            }
            let (a, b) = (buttons[0], buttons[1]);
            let (cx, cy) = b.center();
            assert!(!a.contains(cx, cy), "{screen:?} buttons overlap");
            assert!(b.contains(cx, cy));
        }
    }
}
