//! Pruebas de estado y navegación de la TUI.

use nas_core::NasPaths;
use nas_exec::Runner;
use nas_tui::{App, Data, Section};

#[test]
fn navega_entre_secciones_con_wrap() {
    let mut app = App::new();
    assert_eq!(app.current(), Section::Dashboard);

    for _ in 0..Section::ALL.len() {
        app.next_section();
    }
    assert_eq!(app.current(), Section::Dashboard);

    app.previous_section();
    assert_eq!(app.current(), Section::Quit);
}

#[test]
fn refresh_carga_metricas() {
    let mut app = App::new();
    app.refresh(&Runner::new(), &NasPaths::development());
    assert!(matches!(app.data, Data::Dashboard(_)));
    assert!(app.status.contains("Dashboard"));
}

#[test]
fn seccion_salir_marca_cierre() {
    let mut app = App::new();
    while app.current() != Section::Quit {
        app.next_section();
    }
    app.refresh(&Runner::new(), &NasPaths::development());
    assert!(app.should_quit);
}

#[test]
fn seleccion_respeta_limites() {
    let mut app = App::new();
    app.move_down(0);
    assert_eq!(app.selected, 0);
    app.move_down(3);
    app.move_down(3);
    app.move_down(3);
    assert_eq!(app.selected, 2);
    app.move_up();
    assert_eq!(app.selected, 1);
    app.move_up();
    app.move_up();
    assert_eq!(app.selected, 0);
}

#[test]
fn explorador_rechaza_escapes() {
    let root = std::env::temp_dir().join(format!("nas-tui-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("dir");

    assert!(nas_tui::app::list_files(&root, "../etc").is_err());
    assert!(nas_tui::app::list_files(&root, "/etc").is_err());

    std::fs::create_dir_all(root.join("SISTEMAS")).expect("sub");
    std::fs::write(root.join("a.txt"), b"x").expect("file");
    let entries = nas_tui::app::list_files(&root, "").expect("list");
    assert_eq!(entries[0].name, "SISTEMAS");
    assert_eq!(entries[1].name, "a.txt");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn subir_directorio_actualiza_ruta() {
    let mut app = App::new();
    app.section = Section::ALL
        .iter()
        .position(|section| *section == Section::Files)
        .expect("files");
    app.path = "SISTEMAS/2026".to_string();
    app.go_up();
    assert_eq!(app.path, "SISTEMAS");
    app.go_up();
    assert_eq!(app.path, "");
}
