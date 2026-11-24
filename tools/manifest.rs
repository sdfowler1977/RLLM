//! Renders the variant manifest exactly as package.sh's render_manifest does, for
//! build hosts without sed. `manifest --package FILE` prints the package identity
//! the tree ships; `manifest --render VARIANT CODE FILE OUT` writes the manifest a
//! build of that variant carries.
use std::process::ExitCode;

fn base_package(source: &str) -> Option<String> {
    let line = source.lines().find(|line| line.trim_start().starts_with("<manifest "))?;
    let rest = &line[line.find(" package=\"")? + " package=\"".len()..];
    let name = &rest[..rest.find('"')?];
    (!name.is_empty()).then(|| name.to_string())
}

/// `sed s/from/to/` on one line: the first occurrence only.
fn first(line: &str, from: &str, to: &str) -> String {
    line.replacen(from, to, 1)
}

/// `sed s/NAME="[^"]*"/NAME="value"/` on one line.
fn attribute(line: &str, name: &str, value: &str) -> String {
    let opening = format!("{name}=\"");
    let Some(at) = line.find(&opening) else { return line.to_string() };
    let start = at + opening.len();
    let Some(length) = line[start..].find('"') else { return line.to_string() };
    format!("{}{}{}", &line[..start], value, &line[start + length..])
}

fn render(source: &str, variant: &str, code: u64) -> Result<String, String> {
    let base = base_package(source).ok_or("The package identity cannot be read from the manifest")?;
    let lab = match variant {
        "lab" => true,
        "main" => false,
        _ => return Err("Only the existing main or Lab application can be built.".into()),
    };
    let package = if lab { format!("{base}.lab") } else { base.clone() };
    let name = format!("0.1-{variant}.{code}");
    let mut out = String::new();
    for line in source.lines() {
        let mut line = line.to_string();
        if lab {
            line = first(&line, &format!("package=\"{base}\""), &format!("package=\"{package}\""));
            line = first(&line, "android:label=\"RLLM\"", "android:label=\"RLLM\"");
            for key in ["android:name", "android:targetActivity", "android:parentActivityName"] {
                line = line.replace(&format!("{key}=\"."), &format!("{key}=\"{base}."));
            }
            line = first(&line, &format!("android:scheme=\"{base}.oauth\""), &format!("android:scheme=\"{package}.oauth\""));
        }
        line = attribute(&line, "android:versionCode", &code.to_string());
        line = attribute(&line, "android:versionName", &name);
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [flag, file] if flag == "--package" => {
            let source = std::fs::read_to_string(file).map_err(|error| format!("{file}: {error}"))?;
            println!("{}", base_package(&source).ok_or("The package identity cannot be read from the manifest")?);
            Ok(())
        }
        [flag, variant, code, file, out] if flag == "--render" => {
            let code: u64 = code.parse().map_err(|_| "Version code must be 1..2100000000")?;
            if code == 0 || code > 2_100_000_000 {
                return Err("Version code must be 1..2100000000".into());
            }
            let source = std::fs::read_to_string(file).map_err(|error| format!("{file}: {error}"))?;
            std::fs::write(out, render(&source, variant, code)?).map_err(|error| format!("{out}: {error}"))
        }
        _ => Err("usage: manifest --package FILE | manifest --render VARIANT CODE FILE OUT".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
