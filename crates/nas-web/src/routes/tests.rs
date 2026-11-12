use crate::routes::{clean_helper_error, safe_content_disposition};

#[test]
fn content_disposition_formatea_ascii_y_rfc6266_unicode() {
    let h1 = safe_content_disposition("attachment", "reporte.txt");
    assert_eq!(
        h1.to_str().expect("str"),
        "attachment; filename=\"reporte.txt\"; filename*=UTF-8''reporte.txt"
    );

    let h2 = safe_content_disposition("attachment", "año_2026.pdf");
    assert_eq!(
        h2.to_str().expect("str"),
        "attachment; filename=\"a_o_2026.pdf\"; filename*=UTF-8''a%C3%B1o_2026.pdf"
    );

    let h3 = safe_content_disposition("inline", "música y diseño.mp3");
    assert_eq!(
        h3.to_str().expect("str"),
        "inline; filename=\"m_sica y dise_o.mp3\"; filename*=UTF-8''m%C3%BAsica%20y%20dise%C3%B1o.mp3"
    );

    let h4 = safe_content_disposition("attachment", "\"peligroso\r\n.bin\"");
    let s4 = h4.to_str().expect("str");
    assert!(!s4.contains('\r') && !s4.contains('\n'));

    let h5 = safe_content_disposition("attachment", "");
    assert_eq!(
        h5.to_str().expect("str"),
        "attachment; filename=\"archivo.bin\"; filename*=UTF-8''"
    );
}

#[test]
fn clean_helper_error_desempaqueta_json_y_humaniza_mount_cifs() {
    let json_err = "{\n  \"success\": false,\n  \"message\": \"operación denegada: falló conexión CIFS: mount error(13): Permission denied\\nRefer to the mount.cifs(8) manual page (e.g. man mount.cifs) and kernel log messages (dmesg)\",\n  \"error\": \"operación denegada: falló conexión CIFS: mount error(13): Permission denied\\nRefer to the mount.cifs(8) manual page (e.g. man mount.cifs) and kernel log messages (dmesg)\"\n}";
    let res = clean_helper_error(json_err);
    assert!(res.contains("Acceso denegado"));
    assert!(!res.contains("Refer to the mount.cifs"));
    assert!(!res.contains("\"success\""));

    let host_down = "{\n  \"success\": false,\n  \"message\": \"operación denegada: falló conexión CIFS: mount error(112): Host is down\"\n}";
    let res2 = clean_helper_error(host_down);
    assert!(res2.contains("Host no disponible"));

    let plain = "error desconocido en ejecución";
    assert_eq!(clean_helper_error(plain), "error desconocido en ejecución");
}

mod semver_tests {
    use crate::routes::system::parse_semver;

    #[test]
    fn parsea_versiones_comunes() {
        assert_eq!(parse_semver("1.2.3"), vec![1, 2, 3]);
        assert_eq!(parse_semver("v1.2.3"), vec![1, 2, 3]);
        assert_eq!(parse_semver("0.1.9"), vec![0, 1, 9]);
    }

    #[test]
    fn maneja_versiones_con_sufijo() {
        assert_eq!(parse_semver("1.2.3-beta.1"), vec![1, 2, 3]);
    }
}

mod comparison_tests {
    use crate::routes::system::is_newer_version;

    #[test]
    fn detecta_versions_superiores() {
        assert!(is_newer_version("1.3.0", "1.2.9"));
        assert!(is_newer_version("2.0.0", "1.9.9"));
        assert!(is_newer_version("1.0.0", "0.9.9"));
    }

    #[test]
    fn rechaza_versions_iguales_y_inferiores() {
        assert!(!is_newer_version("1.2.3", "1.2.3"));
        assert!(!is_newer_version("1.2.2", "1.2.3"));
        assert!(!is_newer_version("0.1.9", "0.1.10"));
    }

    #[test]
    fn maneja_diferentes_longitudes() {
        assert!(is_newer_version("1.2", "1.1.9"));
        assert!(!is_newer_version("1.2", "1.2.5"));
    }
}
