use serde_json::{Value, json};
use std::io::{self, BufRead, IsTerminal, Write};

mod logo;
mod terminal;
use terminal::Terminal;

pub(super) fn requested(args: &[String]) -> Option<(&[String], bool)> {
    if args.is_empty() && io::stdin().is_terminal() && io::stdout().is_terminal() {
        return Some((args, false));
    }
    let (state, command) = if args.len() >= 3 && args[0] == "--state" {
        args.split_at(2)
    } else {
        (&[][..], args)
    };
    match command {
        [name] if name == "console" => Some((state, false)),
        [name, flag] if name == "console" && flag == "--no-motion" => Some((state, true)),
        _ => None,
    }
}

pub(super) async fn open(state: &[String], no_motion: bool) -> io::Result<()> {
    let terminal = Terminal::open(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        no_motion,
    );
    let mut input = io::stdin().lock();
    let mut out = io::stdout().lock();
    logo::write(&mut out, &terminal)?;
    if std::env::var_os("VOLDIQ_AGENT_TOKEN").is_none() {
        writeln!(
            out,
            "  For disk access, set VOLDIQ_AGENT_TOKEN from Voldiq Settings."
        )?;
        writeln!(out)?;
    }
    menu(&mut out, &terminal)?;

    let prompt = format!("\n  {}Voldiq >{} ", terminal.accent, terminal.reset);
    while let Some(choice) = ask(&mut input, &mut out, &prompt)? {
        let mut args = state.to_vec();
        match choice.as_str() {
            "0" => break,
            "?" => {
                writeln!(out)?;
                menu(&mut out, &terminal)?;
                continue;
            }
            "r" | "R" => {
                writeln!(out)?;
                logo::write(&mut out, &terminal)?;
                menu(&mut out, &terminal)?;
                continue;
            }
            "h" | "H" => {
                let help = crate::run(&mut Vec::new(), false).await;
                if let Ok(Some(help)) = help {
                    heading(
                        &mut out,
                        &terminal,
                        "Command reference (run from your shell)",
                    )?;
                    writeln!(out)?;
                    for value in items(&help["result"]["commands"]) {
                        writeln!(out, "    voldiq-cli {}", label(value))?;
                    }
                }
                continue;
            }
            "1" => {
                let Some(path) = ask(&mut input, &mut out, "  Folder path > ")? else {
                    break;
                };
                if path.is_empty() {
                    continue;
                }
                args.extend(["scan".into(), path, "--wait".into()]);
            }
            "2" | "3" => {
                let Some(snapshot) =
                    ask(&mut input, &mut out, "  Snapshot ID (see Scan history) > ")?
                else {
                    break;
                };
                if snapshot.is_empty() {
                    continue;
                }
                if choice == "2" {
                    let Some(text) =
                        ask(&mut input, &mut out, "  Name contains (Enter for all) > ")?
                    else {
                        break;
                    };
                    args.extend([
                        "query".into(),
                        snapshot,
                        json!({"text":text,"limit":12}).to_string(),
                    ]);
                } else {
                    args.extend([
                        "duplicate-candidates".into(),
                        snapshot,
                        json!({"limit":12}).to_string(),
                    ]);
                }
            }
            "4" => args.extend(["volumes".into(), "0".into(), "12".into()]),
            "5" => args.extend(["history".into(), "0".into(), "12".into()]),
            "" => continue,
            _ => {
                writeln!(out, "  Choose 1-5, ?, H, R or 0.")?;
                continue;
            }
        }
        match crate::run(&mut args, true).await {
            Ok(Some(output)) => show(&mut out, &terminal, &choice, &output["result"])?,
            Ok(None) => {}
            Err((code, message, _)) => {
                writeln!(out, "  {}", label(&Value::String(message)))?;
                if code == "not_authorized" {
                    writeln!(out, "  In Voldiq, open Settings > Agents and permissions.")?;
                    writeln!(
                        out,
                        "  Set VOLDIQ_AGENT_TOKEN in your shell, then start the console again."
                    )?;
                }
            }
        }
        shortcuts(&mut out, &terminal)?;
    }
    writeln!(out)?;
    Ok(())
}

fn menu(out: &mut impl Write, terminal: &Terminal) -> io::Result<()> {
    let analysis = [
        (1, "Scan a folder"),
        (2, "Search a snapshot"),
        (3, "Duplicate candidates"),
    ];
    let browsing = [(4, "Authorized volumes"), (5, "Scan history")];
    for (section, entries) in [
        ("Analyze", analysis.as_slice()),
        ("Browse", browsing.as_slice()),
    ] {
        heading(out, terminal, section)?;
        for (key, label) in entries {
            writeln!(
                out,
                "    {}[{key}]{}  {label}",
                terminal.accent, terminal.reset
            )?;
        }
        writeln!(out)?;
    }
    shortcuts(out, terminal)
}

fn heading(out: &mut impl Write, terminal: &Terminal, label: &str) -> io::Result<()> {
    writeln!(out, "  {}{label}{}", terminal.strong, terminal.reset)
}

fn shortcuts(out: &mut impl Write, terminal: &Terminal) -> io::Result<()> {
    write!(out, "  ")?;
    for (index, (key, label)) in [
        ('?', "Menu"),
        ('H', "Commands"),
        ('R', "Replay logo"),
        ('0', "Exit"),
    ]
    .into_iter()
    .enumerate()
    {
        if index != 0 {
            write!(out, "   ")?;
        }
        write!(out, "{}[{key}]{} {label}", terminal.accent, terminal.reset)?;
    }
    writeln!(out)
}

fn ask(input: &mut impl BufRead, out: &mut impl Write, prompt: &str) -> io::Result<Option<String>> {
    write!(out, "{prompt}")?;
    out.flush()?;
    let mut answer = String::new();
    let length = input.read_line(&mut answer)?;
    Ok((length != 0).then(|| answer.trim().to_owned()))
}

fn show(out: &mut impl Write, terminal: &Terminal, choice: &str, result: &Value) -> io::Result<()> {
    writeln!(out)?;
    match choice {
        "1" => writeln!(
            out,
            "  Scan #{} complete. Snapshot #{} is ready.",
            label(&result["id"]),
            label(&result["snapshot_id"])
        ),
        "2" => {
            let rows = items(&result["items"]);
            let largest = bytes(&result["largest"]);
            heading(
                out,
                terminal,
                &format!(
                    "Search results  |  {} of {} shown",
                    rows.len(),
                    label(&result["total"])
                ),
            )?;
            for row in rows {
                let size = bytes(&row["logical"]);
                writeln!(
                    out,
                    "  {}  {:>10}  #{}  {}",
                    bar(terminal, size, largest),
                    size_label(size),
                    label(&row["id"]),
                    label(&row["name"])
                )?;
            }
            Ok(())
        }
        "3" => {
            let groups = items(&result["groups"]);
            heading(
                out,
                terminal,
                &format!(
                    "Duplicate candidates  |  {} of {} shown",
                    groups.len(),
                    label(&result["total"])
                ),
            )?;
            writeln!(
                out,
                "  Grouped by recorded size. Content verification is separate."
            )?;
            for group in groups {
                writeln!(
                    out,
                    "  {:>10}  {} distinct files",
                    size_label(bytes(&group["logical_bytes"])),
                    label(&group["distinct_files"])
                )?;
            }
            if result["recorded_complete"].as_bool() == Some(false) {
                writeln!(out, "  Partial snapshot coverage.")?;
            }
            Ok(())
        }
        "4" => {
            heading(out, terminal, "Authorized volumes")?;
            let rows = items(&result["items"]);
            if rows.is_empty() {
                writeln!(out, "  No whole volumes available to this connection.")?;
            }
            for row in rows {
                let measured = &row["space"]["measurement"];
                writeln!(
                    out,
                    "  {}  {}",
                    label(&row["root"]),
                    row["label"].as_str().map(clean).unwrap_or_default()
                )?;
                let total = bytes(&measured["total"]);
                let used = bytes(&measured["used"]);
                writeln!(
                    out,
                    "    {}  {} used / {} total",
                    bar(terminal, used, total),
                    size_label(used),
                    size_label(total)
                )?;
            }
            Ok(())
        }
        _ => {
            heading(out, terminal, "Scan history  |  Snapshot IDs")?;
            let rows = items(&result["items"]);
            if rows.is_empty() {
                writeln!(out, "  No saved snapshots available to this connection.")?;
            }
            for row in rows {
                let coverage = if row["complete"].as_bool() == Some(true) {
                    ""
                } else {
                    "  (partial)"
                };
                writeln!(
                    out,
                    "  #{:<7}  {:>10}  {}{coverage}",
                    label(&row["id"]),
                    size_label(bytes(&row["logical"])),
                    label(&row["root"])
                )?;
            }
            if !result["next_offset"].is_null() {
                writeln!(
                    out,
                    "  More snapshots are available with the history command."
                )?;
            }
            Ok(())
        }
    }
}

fn items(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn label(value: &Value) -> String {
    match value {
        Value::String(text) => clean(text),
        Value::Number(number) => number.to_string(),
        _ => String::from("unknown"),
    }
}

fn clean(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
                '?'
            } else {
                ch
            }
        })
        .collect()
}

fn bytes(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

fn size_label(size: Option<u64>) -> String {
    let Some(size) = size else {
        return "unknown".into();
    };
    let mut count = size as f64;
    for unit in ["B", "KiB", "MiB", "GiB", "TiB", "PiB"] {
        if count < 1024.0 || unit == "PiB" {
            return if unit == "B" {
                format!("{size} B")
            } else {
                format!("{count:.1} {unit}")
            };
        }
        count /= 1024.0;
    }
    unreachable!()
}

fn bar(terminal: &Terminal, part: Option<u64>, total: Option<u64>) -> String {
    const WIDTH: usize = 14;
    match (part, total) {
        (Some(part), Some(total)) if total > 0 && part <= total => {
            let filled = (u128::from(part) * WIDTH as u128 / u128::from(total)) as usize;
            format!(
                "[{}{}{}{}{}{}]",
                terminal.accent,
                "#".repeat(filled),
                terminal.reset,
                terminal.quiet,
                ".".repeat(WIDTH - filled),
                terminal.reset
            )
        }
        _ => format!("[{}]", "-".repeat(WIDTH)),
    }
}
