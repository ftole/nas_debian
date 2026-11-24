use nas_exec::Runner;

use super::manage::toggle_usermod_steps;
use super::samba::{chpasswd_payload, smbpasswd_payload};
use super::*;

#[test]
fn validacion_de_usuarios() {
    assert!(is_valid_user("juan"));
    assert!(is_valid_user("operador_1"));
    assert!(!is_valid_user("root"));
    assert!(!is_valid_user("nas"));
    assert!(!is_valid_user("nas-web"));
    assert!(!is_valid_user("systemd-coredump"));
    assert!(!is_valid_user("Juan"));
    assert!(!is_valid_user(""));
}

#[test]
fn validacion_de_contrasenas_rechaza_cortas_y_control() {
    assert!(validate_password("ClaveSegura1!").is_ok());
    assert!(validate_password("abc:def").is_ok());
    assert!(validate_password("12345").is_err());
    assert!(validate_password("Clave\nroot:hack").is_err());
    assert!(validate_password("Clave\r123").is_err());
    assert!(validate_password("Clave\x00123").is_err());
    assert!(validate_password("Clave\t123").is_err());
}

#[test]
fn create_user_rechaza_nas_y_contrasena_con_salto() {
    let runner = Runner::new();
    let err_nas = create_user(
        &runner,
        &CreateUserArgs {
            username: "nas".into(),
            full_name: "Operador".into(),
            password: "ClaveSegura1!".into(),
            admin: false,
            web: false,
            samba: false,
            groups: vec![],
        },
    )
    .unwrap_err();
    assert!(err_nas.to_string().contains("reservado"));

    let err_ctrl = create_user(
        &runner,
        &CreateUserArgs {
            username: "operador_test".into(),
            full_name: "Operador".into(),
            password: "Clave123\nroot:evil".into(),
            admin: false,
            web: false,
            samba: false,
            groups: vec![],
        },
    )
    .unwrap_err();
    assert!(err_ctrl.to_string().contains("caracteres de control"));
}

#[test]
fn update_user_rechaza_nas_y_contrasena_invalida() {
    let runner = Runner::new();
    let err_nas = update_user(
        &runner,
        &UpdateUserArgs {
            username: "nas".into(),
            full_name: "Nas".into(),
            password: None,
            admin: false,
            web: false,
            samba: false,
            groups: vec![],
        },
    )
    .unwrap_err();
    assert!(err_nas.to_string().contains("reservado"));

    let err_root_ctrl = update_user(
        &runner,
        &UpdateUserArgs {
            username: "root".into(),
            full_name: "Root".into(),
            password: Some("Clave\n123".into()),
            admin: false,
            web: false,
            samba: false,
            groups: vec![],
        },
    )
    .unwrap_err();
    assert!(err_root_ctrl.to_string().contains("reservado"));
}

#[test]
fn payloads_no_agregan_salto_final_redundante_ni_permiten_inyeccion() {
    assert_eq!(
        chpasswd_payload("juan", "Clave123!").unwrap(),
        "juan:Clave123!"
    );
    assert_eq!(
        smbpasswd_payload("Clave123!").unwrap(),
        "Clave123!\nClave123!"
    );
    assert!(chpasswd_payload("juan", "abc\nroot:evil").is_err());
    assert!(chpasswd_payload("juan", "abc\r123").is_err());
    assert!(chpasswd_payload("juan", "abc\x00123").is_err());
    assert!(chpasswd_payload("juan:root", "Clave123!").is_err());
    assert!(smbpasswd_payload("abc\n123").is_err());
    assert!(smbpasswd_payload("abc\r123").is_err());
    assert!(smbpasswd_payload("abc\x00123").is_err());
}

#[test]
fn validacion_de_grupos() {
    assert!(is_valid_group("grp_sistemas"));
    assert!(is_valid_group("grp_contabilidad"));
    assert!(!is_valid_group("sistemas"));
    assert!(!is_valid_group("grp_"));
    assert!(!is_valid_group(
        "grp_nombre_demasiado_largo_que_excede_treinta_caracteres"
    ));
}

#[test]
fn grupos_protegidos_no_se_pueden_eliminar() {
    let runner = Runner::new();
    let err = delete_group(&runner, "grp_superadmin").unwrap_err();
    assert!(err.to_string().contains("está protegido"));
    let err2 = rename_group(&runner, "grp_samba", "grp_otro").unwrap_err();
    assert!(err2.to_string().contains("está protegido"));
}

#[test]
fn sanitizacion_gecos() {
    assert_eq!(sanitize_gecos("Juan:Pérez\r\n"), "JuanPérez");
}

#[test]
fn parsea_salida_pdbedit_w() {
    let sample = "\
operador1:1001:XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX:ABCDEF0123456789:[U          ]:LCT-65000000:
suspendido:1002:XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX:ABCDEF0123456789:[DU         ]:LCT-65000000:
";
    let map = parse_pdbedit_w(sample);
    assert_eq!(map.get("operador1"), Some(&true));
    assert_eq!(map.get("suspendido"), Some(&false));
    assert_eq!(map.get("inexistente"), None);
}

#[test]
fn toggle_usermod_siempre_incluye_usuario() {
    let enable_steps = toggle_usermod_steps("operador1", true);
    assert_eq!(
        enable_steps,
        vec![
            vec!["-e".to_string(), String::new(), "operador1".to_string()],
            vec!["-U".to_string(), "operador1".to_string()],
        ]
    );
    let disable_steps = toggle_usermod_steps("operador1", false);
    assert_eq!(
        disable_steps,
        vec![
            vec!["-L".to_string(), "operador1".to_string()],
            vec!["-e".to_string(), "1".to_string(), "operador1".to_string()],
        ]
    );
}
