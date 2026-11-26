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
    let menu = menu(&terminal, std::env::var_os("VOLDIQ_AGENT_TOKEN").is_some());
    let mut home = Some(logo::write(&mut out, &terminal, &menu, MENU_WIDTH)?);
    loop {
        let menu_column = home.as_ref().map_or(2, |layout| layout.menu_column);
        write!(
            out,
            "\n{:menu_column$}{}Voldiq >{} ",
            "", terminal.accent, terminal.reset
        )?;
        out.flush()?;
        if let Some(layout) = home.take() {
            terminal.spin_idle(&layout);
        }
        let Some(choice) = read_answer(&mut input)? else {
            break;
        };
        let mut args = state.to_vec();
        match choice.as_str() {
            "0" => break,
            "?" => {
                writeln!(out)?;
                home = Some(logo::write(&mut out, &terminal, &menu, MENU_WIDTH)?);
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
            "2" | "3" | "6" => {
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
                } else if choice == "3" {
                    let Some(size) = ask(
                        &mut input,
                        &mut out,
                        "  Exact size in bytes (Enter to list groups) > ",
                    )?
                    else {
                        break;
                    };
                    let options = if size.is_empty() {
                        json!({"limit":12})
                    } else {
                        json!({"logical_bytes":size,"limit":12})
                    };
                    args.extend(["duplicate-candidates".into(), snapshot, options.to_string()]);
                } else {
                    let Some(selection) = ask(
                        &mut input,
                        &mut out,
                        "  File IDs from a snapshot, separated by spaces > ",
                    )?
                    else {
                        break;
                    };
                    if selection.is_empty() {
                        continue;
                    }
                    let ids = selection
                        .split_whitespace()
                        .map(str::parse::<u32>)
                        .collect::<Result<Vec<_>, _>>();
                    let Ok(ids) = ids else {
                        writeln!(out, "  Enter numeric file IDs without the # prefix.")?;
                        continue;
                    };
                    args.extend(["verify-duplicates".into(), snapshot, json!(ids).to_string()]);
                }
            }
            "4" => args.extend(["volumes".into(), "0".into(), "12".into()]),
            "5" => args.extend(["history".into(), "0".into(), "12".into()]),
            "7" | "8" => {
                let (command, id_prompt, field, field_prompt) = if choice == "7" {
                    (
                        "duplicate-analysis",
                        "  Snapshot ID (see Scan history) > ",
                        "scope",
                        "  Folder ID (Enter for the scan root) > ",
                    )
                } else {
                    (
                        "duplicate-results",
                        "  Duplicate analysis job ID > ",
                        "group",
                        "  Group number (Enter to list groups) > ",
                    )
                };
                let Some(id) = ask(&mut input, &mut out, id_prompt)? else {
                    break;
                };
                if id.is_empty() {
                    continue;
                }
                let Some(scope) = ask(&mut input, &mut out, field_prompt)? else {
                    break;
                };
                let mut options = json!({});
                if !scope.is_empty() {
                    let Ok(value) = scope.parse::<u32>() else {
                        writeln!(out, "  Enter a numeric {field} ID.")?;
                        continue;
                    };
                    options[field] = json!(value);
                }
                if choice == "8" {
                    options["limit"] = json!(12);
                }
                args.extend([command.into(), id, options.to_string()]);
            }
            "" => continue,
            _ => {
                writeln!(out, "  Choose 1-8, ?, H, R or 0.")?;
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
        writeln!(out, "  {}", shortcuts(&terminal))?;
    }
    writeln!(out)?;
    Ok(())
}

const MENU_WIDTH: usize = 39;

fn menu(terminal: &Terminal, has_token: bool) -> Vec<String> {
    let Terminal {
        accent,
        reset,
        strong,
        ..
    } = *terminal;
    let mut lines = vec![
        format!("{strong}VOLDIQ  /  Disk space analyzer{reset}"),
        String::new(),
    ];
    let analysis = [
        (1, "Scan a folder"),
        (2, "Search a snapshot"),
        (3, "Duplicate candidates"),
        (6, "Verify exact duplicates"),
        (7, "Analyze a folder for duplicates"),
    ];
    let browsing = [
        (4, "Authorized volumes"),
        (5, "Scan history"),
        (8, "Duplicate analysis results"),
    ];
    for (section, entries) in [
        ("Analyze", analysis.as_slice()),
        ("Browse", browsing.as_slice()),
    ] {
        lines.push(format!("{strong}{section}{reset}"));
        for (key, label) in entries {
            lines.push(format!("  {accent}[{key}]{reset}  {label}"));
        }
        lines.push(String::new());
    }
    lines.push(shortcuts(terminal));
    if !has_token {
        lines.extend([
            String::new(),
            "Disk access needs VOLDIQ_AGENT_TOKEN.".into(),
            "Get it from Voldiq Settings.".into(),
        ]);
    }
    lines
}

fn heading(out: &mut impl Write, terminal: &Terminal, label: &str) -> io::Result<()> {
    writeln!(out, "  {}{label}{}", terminal.strong, terminal.reset)
}

fn shortcuts(terminal: &Terminal) -> String {
    let mark = |key| format!("{}[{key}]{}", terminal.accent, terminal.reset);
    format!(
        "{} Menu   {} Commands   {} Exit",
        mark('?'),
        mark('H'),
        mark('0')
    )
}

fn ask(input: &mut impl BufRead, out: &mut impl Write, prompt: &str) -> io::Result<Option<String>> {
    write!(out, "{prompt}")?;
    out.flush()?;
    read_answer(input)
}

fn read_answer(input: &mut impl BufRead) -> io::Result<Option<String>> {
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
            if result["kind"] == "members" {
                let members = items(&result["members"]);
                heading(
                    out,
                    terminal,
                    &format!(
                        "Candidates at {} bytes  |  {} shown",
                        label(&result["logical_bytes"]),
                        members.len()
                    ),
                )?;
                for member in members {
                    writeln!(
                        out,
                        "  #{:<8}  {}",
                        label(&member["node_id"]),
                        label(&member["name"])
                    )?;
                }
                return writeln!(out, "  [6] Verify selected file IDs by content.");
            }
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
                    "  {:>10}  {} distinct files  |  {} bytes",
                    size_label(bytes(&group["logical_bytes"])),
                    label(&group["distinct_files"]),
                    label(&group["logical_bytes"])
                )?;
            }
            if result["recorded_complete"].as_bool() == Some(false) {
                writeln!(out, "  Partial snapshot coverage.")?;
            }
            Ok(())
        }
        "6" => {
            let groups = items(&result["groups"]);
            heading(out, terminal, "Exact duplicates  |  Selected files")?;
            if groups.is_empty() {
                writeln!(out, "  No matching contents in this selection.")?;
            }
            for group in groups {
                let ids = items(&group["node_ids"])
                    .iter()
                    .map(|node| format!("#{}", label(node)))
                    .collect::<Vec<_>>();
                writeln!(
                    out,
                    "  {:>10}  {}",
                    size_label(bytes(&group["logical_bytes"])),
                    ids.join("  ")
                )?;
            }
            writeln!(
                out,
                "  {} files compared  |  {} hard-link aliases omitted",
                label(&result["compared_files"]),
                label(&result["aliases_omitted"])
            )
        }
        "7" => {
            heading(
                out,
                terminal,
                &format!("Duplicate analysis #{} complete", label(&result["id"])),
            )?;
            writeln!(out, "  [8] Open the groups and file IDs for this job.")
        }
        "8" => {
            heading(
                out,
                terminal,
                &format!(
                    "Duplicate analysis #{}  |  Verified results",
                    label(&result["job_id"])
                ),
            )?;
            if result["kind"] == "members" {
                for node in items(&result["node_ids"]) {
                    writeln!(out, "  File #{}", label(node))?;
                }
            } else {
                let rows = items(&result["groups"]);
                if rows.is_empty() {
                    writeln!(out, "  No verified duplicate groups in the analyzed scope.")?;
                }
                for group in rows {
                    writeln!(
                        out,
                        "  Group {:<6}  {:>10}  {} files",
                        label(&group["group"]),
                        size_label(bytes(&group["logical_bytes"])),
                        label(&group["distinct_files"])
                    )?;
                }
            }
            if result["recorded_complete"].as_bool() == Some(false)
                || result["unavailable_files"]
                    .as_u64()
                    .is_some_and(|count| count > 0)
            {
                writeln!(
                    out,
                    "  Partial evidence  |  {} unavailable files",
                    label(&result["unavailable_files"])
                )?;
            }
            if !result["next_offset"].is_null() {
                writeln!(
                    out,
                    "  More rows are available with the duplicate-results command."
                )?;
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
