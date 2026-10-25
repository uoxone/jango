//! `jango` — create a project and run it.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

const CARGO_TMPL: &str = include_str!("../templates/Cargo.toml.in");
const BUILD_TMPL: &str = include_str!("../templates/build.rs.in");
const MAIN_TMPL: &str = include_str!("../templates/main.rs.in");
const SETTINGS_TMPL: &str = include_str!("../templates/settings.rs.in");
const URLS_TMPL: &str = include_str!("../templates/urls.rs.in");
const VIEWS_TMPL: &str = include_str!("../templates/views.rs.in");
const PAGE_TMPL: &str = include_str!("../templates/page.html.in");
const HOME_TMPL: &str = include_str!("../templates/home.html.in");
const ABOUT_TMPL: &str = include_str!("../templates/about.html.in");
const GITIGNORE_TMPL: &str = include_str!("../templates/gitignore.in");

fn main() {
    let code = match cli() {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("jango: {err}");
            1
        }
    };
    process::exit(code);
}

// -----------------------------------------------
// Commands
//   jango create project <name>
//   jango run [name] [--release]
//   jango build [name]
// -----------------------------------------------

fn cli() -> Result<(), String> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let release = take_flag(&mut args, "--release");
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") | Some("help") => {
            print_help();
            Ok(())
        }
        Some("-V") | Some("--version") | Some("version") => {
            println!("jango {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("create") => create(&args),
        Some("run") => run_project(&args, release),
        Some("build") => build_project(&args),
        Some(other) => Err(format!("unknown command `{other}`\n{}", help_text())),
    }
}

fn print_help() {
    println!("{}", help_text());
}

fn help_text() -> &'static str {
    "\
jango — a small Django-style web framework

Usage:
  jango create project <name>   create ./<name>
  jango run [name] [--release]  run the project (dev build by default)
  jango build [name]            release binary you can upload and run

The file from `jango build` is a normal program.
The server does not need cargo or rustc.

Examples:
  jango create project nava
  jango run nava
  jango build nava"
}

fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    if let Some(index) = args.iter().position(|arg| arg == flag) {
        args.remove(index);
        true
    } else {
        false
    }
}

// -----------------------------------------------
// create project
// Writes a tiny app that depends only on this
// jango crate. templates/ is the first page.
// -----------------------------------------------

fn create(args: &[String]) -> Result<(), String> {
    let Some("project") = args.get(1).map(String::as_str) else {
        return Err("usage: jango create project <name>".into());
    };
    let Some(name) = args.get(2).map(String::as_str) else {
        return Err("usage: jango create project <name>".into());
    };
    if args.len() != 3 {
        return Err("usage: jango create project <name>".into());
    }
    if !valid_name(name) {
        return Err("project name must look like `nava` or `my_nava`".into());
    }

    let dir = PathBuf::from(name);
    if dir.exists() {
        return Err(format!("`{name}` already exists"));
    }
    let lib = jango_lib_path()?;
    let cargo = fill(CARGO_TMPL, name).replace("__JANGO__", &lib);

    fs::create_dir_all(dir.join("src")).map_err(|err| err.to_string())?;
    fs::create_dir_all(dir.join("templates")).map_err(|err| err.to_string())?;
    write(&dir.join("Cargo.toml"), &cargo)?;
    write(&dir.join("build.rs"), BUILD_TMPL)?;
    write(&dir.join("src/main.rs"), MAIN_TMPL)?;
    write(&dir.join("src/settings.rs"), SETTINGS_TMPL)?;
    write(&dir.join("src/urls.rs"), URLS_TMPL)?;
    write(&dir.join("src/views.rs"), &fill(VIEWS_TMPL, name))?;
    write(&dir.join("templates/page.html"), PAGE_TMPL)?;
    write(&dir.join("templates/home.html"), &fill(HOME_TMPL, name))?;
    write(&dir.join("templates/about.html"), &fill(ABOUT_TMPL, name))?;
    write(&dir.join(".gitignore"), GITIGNORE_TMPL)?;

    println!("created {name}");
    println!("  {name}/src/urls.rs");
    println!("  {name}/src/views.rs");
    println!("  {name}/templates/home.html");
    println!();
    println!("next:  jango run {name}");
    Ok(())
}

fn fill(text: &str, name: &str) -> String {
    text.replace("__NAME__", name)
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn jango_lib_path() -> Result<String, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = path.canonicalize().unwrap_or(path);
    let text = path.to_string_lossy().replace('\\', "/");
    if text.contains('"') {
        return Err("jango path cannot contain a quote".into());
    }
    Ok(text)
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|err| format!("write {}: {err}", path.display()))
}

// -----------------------------------------------
// run and build
// `run` compiles fast so you can edit views.
// `build` writes target/release/<name>. Upload
// that file. It does not need cargo on the server.
// -----------------------------------------------

fn run_project(args: &[String], release: bool) -> Result<(), String> {
    let dir = project_dir(args.get(1).map(String::as_str))?;
    if args.len() > 2 {
        return Err("usage: jango run [name] [--release]".into());
    }
    let mut cmd = Command::new("cargo");
    cmd.arg("run").current_dir(&dir);
    if release {
        cmd.arg("--release");
    }
    wait(cmd)
}

fn build_project(args: &[String]) -> Result<(), String> {
    let dir = project_dir(args.get(1).map(String::as_str))?;
    if args.len() > 2 {
        return Err("usage: jango build [name]".into());
    }
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("--release").current_dir(&dir);
    wait(cmd)?;
    let name = package_name(&dir.join("Cargo.toml"))?;
    let binary = dir.join("target/release").join(&name);
    println!();
    println!("binary  {}", binary.display());
    println!("upload this file and run it");
    println!("the server does not need cargo or rustc");
    Ok(())
}

fn project_dir(name: Option<&str>) -> Result<PathBuf, String> {
    let dir = match name {
        Some(name) => PathBuf::from(name),
        None => PathBuf::from("."),
    };
    if !dir.join("Cargo.toml").is_file() {
        return Err(format!("no Cargo.toml in {}", dir.display()));
    }
    Ok(dir)
}

fn package_name(cargo_toml: &Path) -> Result<String, String> {
    let text = fs::read_to_string(cargo_toml).map_err(|err| err.to_string())?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name") {
            let Some((_, value)) = rest.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"');
            if !value.is_empty() {
                return Ok(value.to_string());
            }
        }
    }
    Err("could not find package name in Cargo.toml".into())
}

fn wait(mut cmd: Command) -> Result<(), String> {
    let status = cmd.status().map_err(|err| {
        format!("could not run cargo ({err}). Install Rust on this machine to build.")
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo exited with {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_writes_urls_and_depends_only_on_jango() {
        let root = std::env::temp_dir().join(format!("jango-cli-{}", process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let saved = env::current_dir().unwrap();
        env::set_current_dir(&root).unwrap();
        let result = create(&["create".into(), "project".into(), "nava".into()]);
        env::set_current_dir(saved).unwrap();
        result.unwrap();

        let urls = fs::read_to_string(root.join("nava/src/urls.rs")).unwrap();
        let main_rs = fs::read_to_string(root.join("nava/src/main.rs")).unwrap();
        let views = fs::read_to_string(root.join("nava/src/views.rs")).unwrap();
        let home = fs::read_to_string(root.join("nava/templates/home.html")).unwrap();
        let cargo = fs::read_to_string(root.join("nava/Cargo.toml")).unwrap();
        assert!(urls.contains("routes!"));
        assert!(urls.contains("views::home"));
        assert!(main_rs.contains("mod views;"));
        assert!(main_rs.contains("templates::load"));
        assert!(views.contains("render!(req, \"home.html\""));
        assert!(!views.contains("include_str!"));
        assert!(home.contains("{% extends \"page.html\" %}"));
        assert!(home.contains("<h1>nava</h1>"));
        assert!(fs::read_to_string(root.join("nava/build.rs"))
            .unwrap()
            .contains("templates"));
        assert!(cargo.contains("name = \"nava\""));
        assert!(cargo.contains("jango = { path ="));
        let _ = fs::remove_dir_all(&root);
    }
}
