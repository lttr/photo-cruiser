//! `pcr skill`: a Claude Code skill (`SKILL.md`) that teaches an agent to operate `pcr`.
//! The prose is `skill.md`; the tool table and the command reference are generated, so
//! they follow the code.

use photo_cruiser_core::Tool;

const TEMPLATE: &str = include_str!("skill.md");

/// The skill for `cli`, the `pcr` command.
pub fn markdown(cli: &mut clap::Command) -> String {
    cli.build();
    TEMPLATE
        .replace("{{version}}", env!("CARGO_PKG_VERSION"))
        .replace("{{tools}}", &tools())
        .replace("{{help}}", &help(cli))
}

fn tools() -> String {
    let mut out = String::from(
        "| tool | range | neutral | opens at | what it does |\n| --- | --- | --- | --- | --- |\n",
    );
    for tool in Tool::ALL {
        let (opens, what) = describe(tool);
        let name = serde_json::to_value(tool).expect("tool serialises");
        let range = tool.range();
        out += &format!(
            "| `{}` | {} to {} | {} | {opens} | {}: {what} |\n",
            name.as_str().expect("tool name is a string"),
            range.start(),
            range.end(),
            tool.neutral(),
            tool.label(),
        );
    }
    out.trim_end().to_string()
}

/// The tool's opening value and what it does, for the skill.
fn describe(tool: Tool) -> (&'static str, &'static str) {
    match tool {
        Tool::Levels => (
            "1",
            "per channel, the darkest and brightest 0.5% become black and white (also fixes colour \
             casts). 0–1 blends from the original to that, 1–2 raises the clipping to 5%.",
        ),
        Tool::Exposure => (
            "auto (average brightness to middle grey)",
            "brightness in EV (stops), linear light × 2^value. Applied after levels.",
        ),
        Tool::Colour => (
            "1",
            "colourfulness, darktable's \"basic colorfulness: standard\" × the value, more in the \
             shadows than the highlights. `color` is accepted too.",
        ),
    }
}

/// Every subcommand's `--help`, one section each.
fn help(cli: &clap::Command) -> String {
    let mut out = String::new();
    for sub in cli.get_subcommands().filter(|s| s.get_name() != "help") {
        let text = sub.clone().render_long_help().to_string();
        // Arguments without a description leave a whitespace line and a blank one.
        let mut lines: Vec<&str> = Vec::new();
        for line in text.trim_end().lines().map(str::trim_end) {
            if !(line.is_empty() && lines.last().is_some_and(|l| l.is_empty())) {
                lines.push(line);
            }
        }
        out += &format!("### `pcr {}`\n\n```text\n{}\n```\n\n", sub.get_name(), lines.join("\n"));
    }
    out.trim_end().to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use photo_cruiser_core::EditStack;

    #[test]
    fn covers_every_command_and_tool() {
        let mut cli = crate::Cli::command();
        let md = markdown(&mut cli);
        assert!(md.starts_with("---\nname: pcr\ndescription: "), "frontmatter");
        assert!(!md.contains("{{"), "placeholder left");
        for sub in cli.get_subcommands().filter(|s| s.get_name() != "help") {
            assert!(md.contains(&format!("### `pcr {}`", sub.get_name())), "{}", sub.get_name());
        }
        for tool in Tool::ALL {
            let name = serde_json::to_value(tool).unwrap();
            assert!(md.contains(&format!("| `{}` |", name.as_str().unwrap())), "{tool:?}");
        }
    }

    /// The JSON examples are valid: the edit stack parses, the sidecar is a stack too.
    #[test]
    fn examples_parse() {
        let blocks: Vec<&str> =
            TEMPLATE.split("```json\n").skip(1).map(|b| b.split("```").next().unwrap()).collect();
        assert_eq!(blocks.len(), 2);
        for b in blocks {
            let stack = EditStack::from_json(b).unwrap_or_else(|e| panic!("{e}: {b}"));
            assert!(!stack.ops.is_empty());
        }
    }
}
