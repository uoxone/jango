//! `ongo` — create a project and run it.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

// -----------------------------------------------
// Project files
// Each constant is one file `ongo create` writes.
// `__NAME__` becomes the project name.
// -----------------------------------------------

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
const STYLE_TMPL: &str = include_str!("../templates/style.css.in");

/// Run one command. A problem is printed and the process exits 1.
fn main() {
    let code = match cli() {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("ongo: {err}");
            1
        }
    };
    process::exit(code);
}

// -----------------------------------------------
// Commands
//   ongo create project <name>
//   ongo run [name] [--release]
//   ongo build [name]
// -----------------------------------------------

/// Read the arguments and dispatch `create`, `run`, or `build`.
fn cli() -> Result<(), String> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let release = take_flag(&mut args, "--release");
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") | Some("help") => {
            print_help();
            Ok(())
        }
        Some("-V") | Some("--version") | Some("version") => {
            println!("ongo {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("create") => create(&args),
        Some("run") => run_project(&args, release),
        Some("build") => build_project(&args),
        Some(other) => Err(format!("unknown command `{other}`\n{}", help_text())),
    }
}

/// Print the command list.
fn print_help() {
    println!("{}", help_text());
}

/// The text for `ongo --help`.
fn help_text() -> &'static str {
    "\
ongo — a small Django-style web framework

Usage:
  ongo create project <name>   create ./<name>
  ongo run [name] [--release]  run the project (dev build by default)
  ongo build [name]            binary and static/ in build/

`ongo build` writes build/<name> and build/static.
The server does not need cargo or rustc.

Examples:
  ongo create project nava
  ongo run nava
  ongo build nava"
}

/// Remove `flag` from the arguments. True when it was present.
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
// ongo crate. templates/ is the first page.
// static/ holds CSS, pictures, video, and files.
// -----------------------------------------------

/// Write a new project directory. The name must look like `nava`.
fn create(args: &[String]) -> Result<(), String> {
    let Some("project") = args.get(1).map(String::as_str) else {
        return Err("usage: ongo create project <name>".into());
    };
    let Some(name) = args.get(2).map(String::as_str) else {
        return Err("usage: ongo create project <name>".into());
    };
    if args.len() != 3 {
        return Err("usage: ongo create project <name>".into());
    }
    if !valid_name(name) {
        return Err("project name must look like `nava` or `my_nava`".into());
    }

    let dir = PathBuf::from(name);
    if dir.exists() {
        return Err(format!("`{name}` already exists"));
    }
    let lib = ongo_lib_path()?;
    let cargo = fill(CARGO_TMPL, name).replace("__ONGO__", &lib);

    fs::create_dir_all(dir.join("src")).map_err(|err| err.to_string())?;
    fs::create_dir_all(dir.join("templates")).map_err(|err| err.to_string())?;
    fs::create_dir_all(dir.join("static/css")).map_err(|err| err.to_string())?;
    write(&dir.join("Cargo.toml"), &cargo)?;
    write(&dir.join("build.rs"), BUILD_TMPL)?;
    write(&dir.join("src/main.rs"), MAIN_TMPL)?;
    write(&dir.join("src/settings.rs"), SETTINGS_TMPL)?;
    write(&dir.join("src/urls.rs"), URLS_TMPL)?;
    write(&dir.join("src/views.rs"), &fill(VIEWS_TMPL, name))?;
    write(&dir.join("templates/page.html"), PAGE_TMPL)?;
    write(&dir.join("templates/home.html"), &fill(HOME_TMPL, name))?;
    write(&dir.join("templates/about.html"), &fill(ABOUT_TMPL, name))?;
    write(&dir.join("static/css/style.css"), STYLE_TMPL)?;
    write(&dir.join(".gitignore"), GITIGNORE_TMPL)?;

    println!("created {name}");
    println!("  {name}/src/urls.rs");
    println!("  {name}/src/views.rs");
    println!("  {name}/templates/home.html");
    println!("  {name}/static/css/style.css");
    println!();
    println!("next:  ongo run {name}");
    Ok(())
}

/// Replace `__NAME__` in a template file.
fn fill(text: &str, name: &str) -> String {
    text.replace("__NAME__", name)
}

/// A letter, then letters, digits, `_`, or `-`.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

/// This crate's path, written into the new project's `Cargo.toml`.
fn ongo_lib_path() -> Result<String, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = path.canonicalize().unwrap_or(path);
    let text = path.to_string_lossy().replace('\\', "/");
    if text.contains('"') {
        return Err("ongo path cannot contain a quote".into());
    }
    Ok(text)
}

/// Write one project file. The error includes the path.
fn write(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|err| format!("write {}: {err}", path.display()))
}

// -----------------------------------------------
// run and build
// `run` compiles fast so you can edit views.
// `build` leaves cargo's target/ alone and writes
// a clean build/ folder: the binary and static/.
// -----------------------------------------------

/// `cargo run` in the project. `--release` builds the optimized binary.
fn run_project(args: &[String], release: bool) -> Result<(), String> {
    let dir = project_dir(args.get(1).map(String::as_str))?;
    if args.len() > 2 {
        return Err("usage: ongo run [name] [--release]".into());
    }
    let mut cmd = Command::new("cargo");
    cmd.arg("run").current_dir(&dir);
    if release {
        cmd.arg("--release");
    }
    wait(cmd)
}

/// Release-build the project, then copy the program and `static/` into `build/`.
fn build_project(args: &[String]) -> Result<(), String> {
    let dir = project_dir(args.get(1).map(String::as_str))?;
    if args.len() > 2 {
        return Err("usage: ongo build [name]".into());
    }
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("--release").current_dir(&dir);
    wait(cmd)?;
    let name = package_name(&dir.join("Cargo.toml"))?;
    publish(&dir, &name)
}

/// Replace `build/` with the release binary and, when present, `static/`.
fn publish(dir: &Path, name: &str) -> Result<(), String> {
    let source = dir.join("target/release").join(name);
    if !source.is_file() {
        return Err(format!("no binary at {}", source.display()));
    }
    let out = dir.join("build");
    if out.is_dir() {
        fs::remove_dir_all(&out).map_err(|err| err.to_string())?;
    }
    fs::create_dir_all(&out).map_err(|err| err.to_string())?;
    fs::copy(&source, out.join(name)).map_err(|err| err.to_string())?;
    let static_dir = dir.join("static");
    if static_dir.is_dir() {
        copy_dir(&static_dir, &out.join("static"))?;
    }
    println!();
    println!("build  {}", out.display());
    Ok(())
}

/// Copy a directory, including its subdirectories.
fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|err| err.to_string())?;
    for entry in fs::read_dir(from).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let src = entry.path();
        let dest = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir(&src, &dest)?;
        } else {
            fs::copy(src, dest).map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

/// The named project, or the current directory when the name is omitted.
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

/// The `name` field in `Cargo.toml`. That is the binary's file name.
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

/// Run `cargo`. A missing `cargo` or a failed build becomes an error string.
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
#[path = "../tests/cli.rs"]
mod tests;
