#![forbid(unsafe_code)]

mod journal;
mod transcript;

use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
    process::ExitCode,
};

const USAGE: &str = "Usage: dexdo-deal-inspector <journal|log> <file> [--json]";
const MAX_INPUT: u64 = 16 * 1024 * 1024;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("dexdo-deal-inspector: {message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let (command, file, json) = match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{USAGE}");
            return Ok(());
        }
        [flag] if flag == "--version" => {
            println!("dexdo-deal-inspector {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        [command, file] => (command, file, false),
        [command, file, flag] if flag == "--json" => (command, file, true),
        _ => return Err(USAGE.into()),
    };
    let path = Path::new(file);
    let stdout = io::stdout().lock();
    match command.to_str() {
        Some("journal") => {
            let report = journal::inspect(&read(path)?)?;
            render(stdout, json, &report, |out| report.write_text(out))
        }
        Some("log") => {
            let report = transcript::inspect(&read(path)?);
            render(stdout, json, &report, |out| report.write_text(out))
        }
        _ => Err(USAGE.into()),
    }
}

fn read(path: &Path) -> Result<String, String> {
    if path.as_os_str() == OsStr::new("-") {
        return Err("provide a regular file".into());
    }
    let err = |error: io::Error| match error.kind() {
        io::ErrorKind::NotFound => {
            "file missing; absence alone gives no evidence of the deal outcome".to_owned()
        }
        io::ErrorKind::PermissionDenied => "permission denied reading the input file".into(),
        _ => "could not read input file".into(),
    };
    let info = fs::symlink_metadata(path).map_err(err)?;
    if !info.is_file() {
        return Err("provide a regular file; links and directories are unsupported".into());
    }
    let mut input = Vec::new();
    File::open(path)
        .map_err(err)?
        .take(MAX_INPUT + 1)
        .read_to_end(&mut input)
        .map_err(err)?;
    if input.len() as u64 > MAX_INPUT {
        return Err("input exceeds 16 MiB".into());
    }
    let input = String::from_utf8(input).map_err(|_| "expected a UTF-8 input file".to_owned())?;
    Ok(match input.strip_prefix('\u{feff}') {
        Some(text) => text.to_owned(),
        None => input,
    })
}

fn render(
    mut out: impl Write,
    json: bool,
    report: &impl serde::Serialize,
    write_text: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> Result<(), String> {
    let result = if json {
        serde_json::to_writer_pretty(&mut out, report)
            .map_err(|_| "could not write JSON report")?;
        writeln!(out)
    } else {
        write_text(&mut out)
    };
    result.map_err(|_| "could not write report".into())
}
