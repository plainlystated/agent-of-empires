//! Runtime grafting of plugin-contributed CLI commands into the derived
//! clap tree (D4 of the plugin-system design).
//!
//! Core stays a compile-time clap derive; plugin commands are appended to the
//! derived `Command` per invocation, so disabling a plugin removes its verbs
//! on the next run with no residue. Dispatch first tries the derived enum
//! (core always wins); only when that fails does the plugin registry claim
//! the matched path.

use anyhow::Result;
use aoe_plugin_api::CliCommandContribution;
use clap::{ArgMatches, Command};
use tracing::warn;

/// One grafted command with its owning plugin.
pub struct GraftedCommand {
    pub plugin_id: String,
    pub contribution: CliCommandContribution,
}

/// Collect every active plugin's CLI contributions. Paths that would collide
/// with a core verb are dropped here with a visible warning (core wins,
/// never shadowed); `plugin` is reserved for plugin management.
pub fn grafted_commands(root: &Command) -> Vec<GraftedCommand> {
    let registry = super::registry();
    let mut out: Vec<GraftedCommand> = Vec::new();
    for plugin in registry.active() {
        for contribution in &plugin.manifest.commands {
            let head = &contribution.path[0];
            let collides_core = contribution.path.len() == 1
                && root.get_subcommands().any(|c| c.get_name() == *head);
            let reserved = head == "plugin";
            let collides_plugin = out.iter().any(|g| g.contribution.path == contribution.path);
            if collides_core || reserved || collides_plugin {
                warn!(
                    target: "plugin",
                    plugin = plugin.id(),
                    path = ?contribution.path,
                    "CLI command path unavailable (core-owned, reserved, or already grafted); skipped"
                );
                continue;
            }
            out.push(GraftedCommand {
                plugin_id: plugin.id().to_string(),
                contribution: contribution.clone(),
            });
        }
    }
    out
}

fn build_clap_command(contribution: &CliCommandContribution) -> Command {
    let leaf = contribution.path.last().expect("validated non-empty path");
    let mut cmd = Command::new(leaf.clone()).about(contribution.about.clone());
    for arg in &contribution.args {
        cmd = cmd.arg(
            clap::Arg::new(arg.name.clone())
                .help(arg.help.clone())
                .required(arg.required),
        );
    }
    cmd
}

/// Append every grafted command to the derived root. Nested paths attach
/// under the existing group when it exists (`["session", "archive"]`), or
/// create the intermediate level when it does not.
pub fn graft_all(mut root: Command, commands: &[GraftedCommand]) -> Command {
    for grafted in commands {
        let path = &grafted.contribution.path;
        let leaf_cmd = build_clap_command(&grafted.contribution);
        if path.len() == 1 {
            root = root.subcommand(leaf_cmd);
        } else if path.len() == 2 {
            let group = path[0].clone();
            if root.get_subcommands().any(|c| c.get_name() == group) {
                root = root.mut_subcommand(group, |sub| sub.subcommand(leaf_cmd));
            } else {
                root = root.subcommand(Command::new(group).subcommand(leaf_cmd));
            }
        } else {
            warn!(
                target: "plugin",
                plugin = %grafted.plugin_id,
                path = ?path,
                "CLI paths deeper than two levels are not supported; skipped"
            );
        }
    }
    root
}

/// If the parsed matches landed on a grafted command, dispatch it to the
/// owning plugin's worker and return the outcome. `None` means the matches
/// do not belong to any plugin (the caller surfaces the original derive
/// error).
pub fn dispatch(matches: &ArgMatches, commands: &[GraftedCommand]) -> Option<Result<()>> {
    // Walk the matched subcommand path.
    let mut path: Vec<String> = Vec::new();
    let mut cursor = matches;
    while let Some((name, sub)) = cursor.subcommand() {
        path.push(name.to_string());
        cursor = sub;
    }
    let grafted = commands.iter().find(|g| g.contribution.path == path)?;
    let mut args = serde_json::Map::new();
    for arg in &grafted.contribution.args {
        if let Some(value) = cursor.get_one::<String>(&arg.name) {
            args.insert(arg.name.clone(), serde_json::Value::String(value.clone()));
        }
    }
    let params = serde_json::json!({ "args": args });
    Some(
        super::runtime::invoke_action(&grafted.plugin_id, &grafted.contribution.rpc_method, params)
            .map(|result| {
                if let Some(text) = result.as_str() {
                    println!("{text}");
                } else if !result.is_null() {
                    println!("{result}");
                }
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contribution(path: &[&str]) -> CliCommandContribution {
        CliCommandContribution {
            path: path.iter().map(|s| s.to_string()).collect(),
            about: "test".into(),
            args: vec![],
            rpc_method: "test.run".into(),
        }
    }

    fn grafted(path: &[&str]) -> GraftedCommand {
        GraftedCommand {
            plugin_id: "test-plugin".into(),
            contribution: contribution(path),
        }
    }

    #[test]
    fn top_level_graft_parses_and_dispatch_path_matches() {
        let root = Command::new("aoe").subcommand(Command::new("list"));
        let commands = vec![grafted(&["review"])];
        let root = graft_all(root, &commands);
        let matches = root.try_get_matches_from(["aoe", "review"]).unwrap();
        let mut path = Vec::new();
        let mut cursor = &matches;
        while let Some((name, sub)) = cursor.subcommand() {
            path.push(name.to_string());
            cursor = sub;
        }
        assert_eq!(path, ["review"]);
    }

    #[test]
    fn nested_graft_attaches_under_existing_group() {
        let root =
            Command::new("aoe").subcommand(Command::new("session").subcommand(Command::new("ls")));
        let commands = vec![grafted(&["session", "archive"])];
        let root = graft_all(root, &commands);
        let matches = root
            .try_get_matches_from(["aoe", "session", "archive"])
            .unwrap();
        let (name, sub) = matches.subcommand().unwrap();
        assert_eq!(name, "session");
        assert_eq!(sub.subcommand().unwrap().0, "archive");
    }

    #[test]
    fn dispatch_ignores_core_paths() {
        let commands = vec![grafted(&["review"])];
        let root = Command::new("aoe")
            .subcommand(Command::new("list"))
            .subcommand(Command::new("review"));
        let matches = root.try_get_matches_from(["aoe", "list"]).unwrap();
        assert!(dispatch(&matches, &commands).is_none());
    }
}
