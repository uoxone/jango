use super::*;

#[test]
fn names_flags_and_package_files() {
    assert!(valid_name("nava"));
    assert!(valid_name("my_nava"));
    assert!(valid_name("my-nava"));
    assert!(valid_name("A"));
    for bad in ["", "1nava", "_nava", "na va", "nava!", "nava.x", "-nava"] {
        assert!(!valid_name(bad), "{bad}");
    }

    let mut args = vec!["run".into(), "nava".into(), "--release".into()];
    assert!(take_flag(&mut args, "--release"));
    assert_eq!(args, ["run", "nava"]);
    assert!(!take_flag(&mut args, "--release"));
    assert_eq!(fill("hello __NAME__", "nava"), "hello nava");
    let help = help_text();
    assert!(help.contains("jango create project <name>"));
    assert!(help.contains("jango run [name]"));
    assert!(help.contains("jango build [name]"));

    let dir = std::env::temp_dir().join(format!("jango-pkg-{}", process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let cargo = dir.join("Cargo.toml");
    fs::write(&cargo, "[package]\nname = \"nava\"\n").unwrap();
    assert_eq!(package_name(&cargo).unwrap(), "nava");
    fs::write(&cargo, "name=\"demo\"\n").unwrap();
    assert_eq!(package_name(&cargo).unwrap(), "demo");
    fs::write(&cargo, "version = \"0.1.0\"\n").unwrap();
    assert!(package_name(&cargo).is_err());

    let found = project_dir(Some(dir.to_str().unwrap())).unwrap();
    assert_eq!(found, dir);
    assert!(project_dir(Some(dir.join("missing").to_str().unwrap())).is_err());

    let name = dir.to_str().unwrap().to_string();
    let err = run_project(&["run".into(), name.clone(), "extra".into()], false).unwrap_err();
    assert!(err.contains("usage"));
    let err = build_project(&["build".into(), name, "extra".into()]).unwrap_err();
    assert!(err.contains("usage"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn create_rejects_a_bad_command() {
    for args in [
        &["create"][..],
        &["create", "app", "nava"],
        &["create", "project"],
        &["create", "project", "nava", "extra"],
        &["create", "project", "1bad"],
        &["create", "project", "nava!"],
    ] {
        let owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        assert!(create(&owned).is_err(), "{args:?}");
    }
}

#[test]
fn create_writes_urls_and_depends_only_on_jango() {
    let root = std::env::temp_dir().join(format!("jango-cli-{}", process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let saved = env::current_dir().unwrap();
    env::set_current_dir(&root).unwrap();
    let result = create(&["create".into(), "project".into(), "nava".into()]);
    let again = create(&["create".into(), "project".into(), "nava".into()]);
    env::set_current_dir(&saved).unwrap();
    result.unwrap();
    assert!(again.unwrap_err().contains("already exists"));

    let urls = fs::read_to_string(root.join("nava/src/urls.rs")).unwrap();
    let main_rs = fs::read_to_string(root.join("nava/src/main.rs")).unwrap();
    let views = fs::read_to_string(root.join("nava/src/views.rs")).unwrap();
    let home = fs::read_to_string(root.join("nava/templates/home.html")).unwrap();
    let about = fs::read_to_string(root.join("nava/templates/about.html")).unwrap();
    let settings = fs::read_to_string(root.join("nava/src/settings.rs")).unwrap();
    let cargo = fs::read_to_string(root.join("nava/Cargo.toml")).unwrap();
    assert!(urls.contains("routes!"));
    assert!(urls.contains("views::home"));
    assert!(urls.contains("views::about"));
    assert!(main_rs.contains("mod views;"));
    assert!(main_rs.contains("templates::load"));
    assert!(views.contains("render!(req, \"home.html\""));
    assert!(views.contains("render!(req, \"about.html\""));
    assert!(!views.contains("include_str!"));
    assert!(!views.contains("__NAME__"));
    assert!(home.contains("{% extends \"page.html\" %}"));
    assert!(home.contains("<h1>nava</h1>"));
    assert!(!home.contains("__NAME__"));
    assert!(about.contains("{{ request.path }}"));
    assert!(about.contains("About · nava · Jango"));
    assert!(settings.contains("127.0.0.1:8000"));
    assert!(fs::read_to_string(root.join("nava/.gitignore"))
        .unwrap()
        .contains("target"));
    assert!(fs::read_to_string(root.join("nava/build.rs"))
        .unwrap()
        .contains("templates"));
    assert!(cargo.contains("name = \"nava\""));
    assert!(cargo.contains("jango = { path ="));
    assert!(!cargo.contains("__JANGO__"));
    let _ = fs::remove_dir_all(&root);
}
