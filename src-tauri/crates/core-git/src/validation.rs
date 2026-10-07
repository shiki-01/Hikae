use serde::{Deserialize, Serialize};

/// アプリが ref の作成・更新・削除を行ってよい名前空間
pub const APP_REF_NAMESPACE: &str = "refs/hikae/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rejection {
    pub reason: String,
}

/// git の引数をホワイトリスト方式で検証する
pub fn validate(args: &[&str]) -> Result<(), Rejection> {
    if args.is_empty() {
        return Err(Rejection {
            reason: "empty argument list".to_string(),
        });
    }

    // グローバルオプションと -c の処理
    let (idx, has_c_args) = check_global_options(args)?;

    let mut idx = idx;

    // サブコマンドを取得
    if idx >= args.len() {
        // グローバルオプション（--version など）のみの場合
        return Ok(());
    }

    let subcmd = args[idx];
    idx += 1;

    // サブコマンドを検証
    validate_subcommand(subcmd, &args[idx..], has_c_args)?;

    Ok(())
}

fn check_global_options(args: &[&str]) -> Result<(usize, bool), Rejection> {
    let mut idx = 0;
    let mut has_c_args = false;

    while idx < args.len() {
        let arg = args[idx];

        if arg == "--version" {
            // --version のみ許可
            if idx == 0 && args.len() == 1 {
                return Ok((idx, false));
            } else {
                return Err(Rejection {
                    reason: "--version must be alone".to_string(),
                });
            }
        }

        if arg.starts_with("-c") {
            // -c key=value または -c key value のいずれかの形式を許可
            let key_value = if arg == "-c" {
                // 次の引数が値
                if idx + 1 >= args.len() {
                    return Err(Rejection {
                        reason: "-c requires a key=value argument".to_string(),
                    });
                }
                idx += 1;
                args[idx]
            } else if let Some(rest) = arg.strip_prefix("-c") {
                // -ckey=value 形式
                rest
            } else {
                return Err(Rejection {
                    reason: "invalid -c format".to_string(),
                });
            };

            // key=value をチェック
            if !key_value.contains('=') {
                return Err(Rejection {
                    reason: "-c requires key=value format".to_string(),
                });
            }

            let parts: Vec<&str> = key_value.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(Rejection {
                    reason: "-c requires key=value format".to_string(),
                });
            }

            let key = parts[0];
            validate_config_key(key)?;
            has_c_args = true;
            idx += 1;
            continue;
        }

        if arg.starts_with('-') {
            // グローバルオプションは許可されない
            return Err(Rejection {
                reason: format!("global option '{}' not allowed", arg),
            });
        }

        // サブコマンドの開始
        break;
    }

    Ok((idx, has_c_args))
}

fn validate_config_key(key: &str) -> Result<(), Rejection> {
    let allowed = [
        "core.hooksPath",
        "core.autocrlf",
        "core.precomposeUnicode",
        "core.quotepath",
        "core.longpaths",
        "credential.helper",
        "user.name",
        "user.email",
        "commit.gpgsign",
        "init.defaultBranch",
        "protocol.version",
    ];

    if allowed.contains(&key) {
        Ok(())
    } else {
        Err(Rejection {
            reason: format!("config key '{}' not allowed", key),
        })
    }
}

fn validate_subcommand(cmd: &str, args: &[&str], has_c_args: bool) -> Result<(), Rejection> {
    match cmd {
        "init" => validate_init_args(args),
        "clone" => validate_clone_args(args),
        "config" => validate_config_args(args),
        "add" => validate_add_args(args),
        "rm" => validate_rm_args(args),
        "commit" => validate_commit_args(args, has_c_args),
        "commit-tree" => validate_commit_tree_args(args),
        "write-tree" => validate_write_tree_args(args),
        "read-tree" => validate_read_tree_args(args),
        "update-ref" => validate_update_ref_args(args),
        "status" => validate_status_args(args),
        "diff" => validate_diff_args(args),
        "log" => validate_log_args(args),
        "show" => validate_show_args(args),
        "ls-files" => validate_ls_files_args(args),
        "ls-tree" => validate_ls_tree_args(args),
        "cat-file" => validate_cat_file_args(args),
        "rev-parse" => validate_rev_parse_args(args),
        "rev-list" => validate_rev_list_args(args),
        "for-each-ref" => validate_for_each_ref_args(args),
        "merge-base" => validate_merge_base_args(args),
        "show-ref" => validate_show_ref_args(args),
        "symbolic-ref" => validate_symbolic_ref_args(args),
        "check-ignore" => validate_check_ignore_args(args),
        "ls-remote" => validate_ls_remote_args(args),
        "count-objects" => validate_count_objects_args(args),
        "branch" => validate_branch_args(args),
        "remote" => validate_remote_args(args),
        "fetch" => validate_fetch_args(args),
        "push" => validate_push_args(args),
        "merge" => validate_merge_args(args),
        "checkout" => validate_checkout_args(args),
        "restore" => validate_restore_args(args),

        // Rejected subcommands
        "reset" | "clean" | "rebase" | "filter-branch" | "gc" | "reflog" | "stash" | "tag"
        | "pull" | "cherry-pick" | "revert" | "worktree" | "submodule" => Err(Rejection {
            reason: format!("subcommand '{}' not allowed", cmd),
        }),

        _ => Err(Rejection {
            reason: format!("unknown subcommand '{}'", cmd),
        }),
    }
}

fn validate_init_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed_flags = ["-q", "--quiet", "--initial-branch", "-b"];
    let mut i = 0;

    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg.starts_with("--initial-branch=") {
            i += 1;
            continue;
        }

        if arg == "--initial-branch" || arg == "-b" {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if allowed_flags.contains(&arg) {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        // positional argument (repository path)
        i += 1;
    }

    Ok(())
}

fn validate_clone_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed_flags = [
        "-q",
        "--quiet",
        "--progress",
        "--origin",
        "-o",
        "--branch",
        "-b",
    ];
    let mut i = 0;

    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg.starts_with("--origin=") || arg.starts_with("--branch=") {
            i += 1;
            continue;
        }

        if arg == "--origin" || arg == "-o" || arg == "--branch" || arg == "-b" {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if allowed_flags.contains(&arg) {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        // positional argument
        i += 1;
    }

    Ok(())
}

fn validate_config_args(args: &[&str]) -> Result<(), Rejection> {
    // --local is mandatory
    if args.is_empty() || (args[0] != "--local" && !args[0].starts_with("--local=")) {
        // Check if --global, --system, --file, --worktree, --blob is present
        for arg in args {
            match *arg {
                "--global" | "--system" | "--file" | "-f" | "--worktree" | "--blob" => {
                    return Err(Rejection {
                        reason: format!("'{}' not allowed in this context", arg),
                    });
                }
                _ => {}
            }
        }
        return Err(Rejection {
            reason: "--local is required".to_string(),
        });
    }

    let allowed_flags = [
        "--get",
        "--get-all",
        "--list",
        "-l",
        "-z",
        "--null",
        "--unset",
        "--add",
    ];
    let mut i = 1; // Skip --local

    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if allowed_flags.contains(&arg) {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        // positional argument (key or key value)
        i += 1;
    }

    Ok(())
}

fn validate_add_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed_flags = [
        "-A",
        "--all",
        "-u",
        "--update",
        "-N",
        "--intent-to-add",
        "-q",
        "--quiet",
        "--ignore-errors",
    ];

    for arg in args {
        match *arg {
            "--" => break,
            "-f" | "--force" => {
                return Err(Rejection {
                    reason: format!("'{}' not allowed", arg),
                });
            }
            _ => {}
        }

        if allowed_flags.contains(arg) {
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        // pathspec
    }

    Ok(())
}

fn validate_rm_args(args: &[&str]) -> Result<(), Rejection> {
    if args.is_empty() || args[0] != "--cached" {
        return Err(Rejection {
            reason: "--cached is required".to_string(),
        });
    }

    let allowed_flags = ["--cached", "-r", "-q", "--quiet", "--ignore-unmatch"];

    for arg in args {
        match *arg {
            "--" => break,
            "-f" | "--force" => {
                return Err(Rejection {
                    reason: format!("'{}' not allowed", arg),
                });
            }
            _ => {}
        }

        if allowed_flags.contains(arg) {
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        // pathspec
    }

    Ok(())
}

fn validate_commit_args(args: &[&str], _has_c_args: bool) -> Result<(), Rejection> {
    // If --amend is present, reject
    for arg in args {
        match *arg {
            "--amend" | "-a" | "--all" | "--fixup" | "--squash" | "-C" | "-c"
            | "--reset-author" => {
                return Err(Rejection {
                    reason: format!("'{}' not allowed", arg),
                });
            }
            _ => {}
        }

        // Check for abbreviated forms
        if *arg == "--am" || arg.starts_with("--am=") {
            return Err(Rejection {
                reason: "--amend cannot be abbreviated to --am".to_string(),
            });
        }
    }

    let allowed_flags = [
        "-m",
        "--message",
        "--allow-empty",
        "--no-verify",
        "-q",
        "--quiet",
        "--no-edit",
        "--author",
        "--no-gpg-sign",
    ];

    let mut i = 0;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg.starts_with("-m") && arg.len() > 2 {
            // -m<msg> form
            i += 1;
            continue;
        }

        if arg == "-m" || arg == "--message" {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if arg.starts_with("--message=") {
            i += 1;
            continue;
        }

        if arg.starts_with("--author=") {
            i += 1;
            continue;
        }

        if arg == "--author" {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if allowed_flags.contains(&arg) {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        i += 1;
    }

    Ok(())
}

fn validate_commit_tree_args(args: &[&str]) -> Result<(), Rejection> {
    let mut i = 0;

    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg == "-p" || arg == "-m" {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        // positional argument
        i += 1;
    }

    Ok(())
}

fn validate_write_tree_args(args: &[&str]) -> Result<(), Rejection> {
    for arg in args {
        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_read_tree_args(args: &[&str]) -> Result<(), Rejection> {
    for arg in args {
        if *arg == "--empty" {
            return Ok(());
        }
        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_update_ref_args(args: &[&str]) -> Result<(), Rejection> {
    // 更新・作成・削除のいずれも、対象 ref は自アプリの名前空間に限る（不変条件 5 を拡張）。
    // 「全ゼロの OID を new value にする」削除や、HEAD / refs/heads/ の付け替えも同時に塞ぐ。
    let mut positional: Vec<&str> = Vec::new();
    let mut i = 0;
    let mut after_dd = false;
    while i < args.len() {
        let arg = args[i];
        if after_dd {
            positional.push(arg);
            i += 1;
            continue;
        }
        match arg {
            "--" => after_dd = true,
            "-d" | "--no-deref" | "--create-reflog" => {}
            "-m" => {
                if i + 1 >= args.len() {
                    return Err(Rejection {
                        reason: "'-m' requires a value".to_string(),
                    });
                }
                i += 1;
            }
            _ if arg.starts_with('-') => {
                return Err(Rejection {
                    reason: format!("flag '{arg}' not allowed"),
                });
            }
            _ => positional.push(arg),
        }
        i += 1;
    }

    let Some(target) = positional.first() else {
        return Err(Rejection {
            reason: "update-ref requires a ref name".to_string(),
        });
    };
    if !target.starts_with(APP_REF_NAMESPACE) || target.contains("..") {
        return Err(Rejection {
            reason: format!(
                "update-ref is only allowed for refs under '{APP_REF_NAMESPACE}', got '{target}'"
            ),
        });
    }
    // 位置引数は <ref> [<newvalue> [<oldvalue>]]。値は 40 / 64 桁の 16 進 OID に限り、
    // 削除を意味する全ゼロ OID は拒否する
    let is_oid =
        |v: &&str| (v.len() == 40 || v.len() == 64) && v.chars().all(|c| c.is_ascii_hexdigit());
    let rest = &positional[1..];
    if rest.len() > 2
        || !rest.iter().all(is_oid)
        || rest.iter().any(|v| v.chars().all(|c| c == '0'))
    {
        return Err(Rejection {
            reason: "update-ref values must be non-zero full object ids".to_string(),
        });
    }
    Ok(())
}

fn validate_status_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = [
        "--porcelain",
        "--porcelain=v2",
        "-z",
        "--branch",
        "-b",
        "--untracked-files",
        "-u",
        "--untracked-files=all",
        "--untracked-files=normal",
        "--untracked-files=no",
        "-uall",
        "-unormal",
        "-uno",
        "--ignored",
        "--no-renames",
        "--ahead-behind",
        "--no-ahead-behind",
    ];

    for arg in args {
        if *arg == "--" {
            break;
        }

        if !allowed.contains(arg)
            && !arg.starts_with("--porcelain=")
            && !arg.starts_with("--untracked-files=")
            && arg.starts_with('-')
        {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_diff_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = [
        "-z",
        "--name-status",
        "--name-only",
        "--numstat",
        "--raw",
        "--cached",
        "--staged",
        "--no-renames",
        "--find-renames",
        "-M",
        "--unified",
        "-U",
        "--binary",
        "--no-color",
        "--color=never",
        "--stat",
        "--quiet",
        "--exit-code",
        "--diff-filter",
        "--no-ext-diff",
        "--no-textconv",
        "--patch",
        "-p",
        "--shortstat",
        "--text",
        "-a",
    ];

    let mut i = 0;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg.starts_with("--unified=") || arg.starts_with("--diff-filter=") {
            i += 1;
            continue;
        }

        if arg == "--unified" || arg == "-U" {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if allowed
            .iter()
            .any(|&f| arg == f || arg.starts_with(&format!("{}-", f)))
        {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        i += 1;
    }

    Ok(())
}

fn validate_log_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = [
        "-z",
        "--format",
        "--pretty",
        "-n",
        "--max-count",
        "--no-color",
        "--follow",
        "--all",
        "--skip",
        "--date",
        "--no-renames",
        "--name-status",
        "--name-only",
        "--reverse",
        "--first-parent",
        "--oneline",
        "--parents",
    ];

    let mut i = 0;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg.starts_with("--format=")
            || arg.starts_with("--pretty=")
            || arg.starts_with("--max-count=")
            || arg.starts_with("--skip=")
            || arg.starts_with("--date=")
            || arg.starts_with("-n")
        {
            i += 1;
            continue;
        }

        if arg == "--format"
            || arg == "--pretty"
            || arg == "--max-count"
            || arg == "-n"
            || arg == "--skip"
            || arg == "--date"
        {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if allowed.contains(&arg) {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        i += 1;
    }

    Ok(())
}

fn validate_show_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = [
        "-z",
        "--format",
        "--pretty",
        "-n",
        "--max-count",
        "--no-color",
        "--follow",
        "--all",
        "--skip",
        "--date",
        "--no-renames",
        "--name-status",
        "--name-only",
        "--reverse",
        "--first-parent",
        "--oneline",
        "--parents",
        "--stat",
        "--no-patch",
        "-s",
    ];

    let mut i = 0;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg.starts_with("--format=")
            || arg.starts_with("--pretty=")
            || arg.starts_with("--max-count=")
            || arg.starts_with("--skip=")
            || arg.starts_with("--date=")
            || arg.starts_with("-n")
        {
            i += 1;
            continue;
        }

        if arg == "--format"
            || arg == "--pretty"
            || arg == "--max-count"
            || arg == "-n"
            || arg == "--skip"
            || arg == "--date"
        {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if allowed.contains(&arg) {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        i += 1;
    }

    Ok(())
}

fn validate_ls_files_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = [
        "-z",
        "-c",
        "--cached",
        "-o",
        "--others",
        "-s",
        "--stage",
        "-u",
        "--unmerged",
        "-m",
        "--modified",
        "-d",
        "--deleted",
        "-i",
        "--ignored",
        "--exclude-standard",
        "--error-unmatch",
    ];

    for arg in args {
        if *arg == "--" {
            break;
        }

        if !allowed.contains(arg) && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_ls_tree_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = ["-z", "-r", "-t", "-l", "--name-only", "--full-tree"];

    for arg in args {
        if *arg == "--" {
            break;
        }

        if !allowed.contains(arg) && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_cat_file_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = ["-p", "-t", "-s", "-e", "--batch", "--batch-check"];

    for arg in args {
        if *arg == "--" {
            break;
        }

        if !allowed.contains(arg) && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_rev_parse_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = [
        "--git-dir",
        "--show-toplevel",
        "--verify",
        "--abbrev-ref",
        "--short",
        "--is-inside-work-tree",
        "--git-path",
        "-q",
        "--quiet",
        "--symbolic-full-name",
    ];

    let mut i = 0;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg.starts_with("--git-path=") {
            i += 1;
            continue;
        }

        if arg == "--git-path" {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if allowed.contains(&arg) {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        i += 1;
    }

    Ok(())
}

fn validate_rev_list_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = [
        "--count",
        "--max-count",
        "-n",
        "--left-right",
        "--parents",
        "--all",
        "--reverse",
    ];

    let mut i = 0;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg.starts_with("--max-count=") || arg.starts_with("-n") {
            i += 1;
            continue;
        }

        if arg == "--max-count" || arg == "-n" {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if allowed.contains(&arg) {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        i += 1;
    }

    Ok(())
}

fn validate_for_each_ref_args(args: &[&str]) -> Result<(), Rejection> {
    let _allowed = ["--format", "--sort", "--count"];

    let mut i = 0;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg.starts_with("--format=") || arg.starts_with("--sort=") || arg.starts_with("--count=")
        {
            i += 1;
            continue;
        }

        if arg == "--format" || arg == "--sort" || arg == "--count" {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        i += 1;
    }

    Ok(())
}

fn validate_merge_base_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = ["--is-ancestor", "--all"];

    for arg in args {
        if *arg == "--" {
            break;
        }

        if !allowed.contains(arg) && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_show_ref_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = ["--verify", "-q", "--quiet", "--heads", "--hash", "-s"];

    for arg in args {
        if *arg == "--" {
            break;
        }

        if !allowed.contains(arg) && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_symbolic_ref_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = ["-q", "--short", "--quiet"];

    // Check if position args >= 2 (write operation)
    if args.len() >= 2 && !args[0].starts_with('-') && !args[1].starts_with('-') {
        return Err(Rejection {
            reason: "symbolic-ref with write operation not allowed".to_string(),
        });
    }

    for arg in args {
        if *arg == "--" {
            break;
        }

        if !allowed.contains(arg) && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_check_ignore_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = ["-v", "-z", "-q", "--no-index"];

    for arg in args {
        if *arg == "--" {
            break;
        }

        if !allowed.contains(arg) && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_ls_remote_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = ["--heads", "--tags", "--refs", "-q"];

    for arg in args {
        if *arg == "--" {
            break;
        }

        if !allowed.contains(arg) && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_count_objects_args(args: &[&str]) -> Result<(), Rejection> {
    let allowed = ["-v"];

    for arg in args {
        if *arg == "--" {
            break;
        }

        if !allowed.contains(arg) && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_branch_args(args: &[&str]) -> Result<(), Rejection> {
    // Only read-only operations allowed
    let not_allowed = ["-d", "-D", "--delete", "-m", "-M", "-c", "-f", "--force"];

    for arg in args {
        if not_allowed.contains(arg) || (arg.starts_with("-f") && arg.len() > 2) {
            return Err(Rejection {
                reason: format!("'{}' not allowed", arg),
            });
        }
    }

    let allowed = [
        "--show-current",
        "--list",
        "-a",
        "--all",
        "-r",
        "--remotes",
        "-v",
        "--format",
    ];

    let mut has_list_flag = false;
    let mut has_positional = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg.starts_with("--format=") {
            i += 1;
            continue;
        }

        if arg == "--format" {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if arg == "--list" || arg == "-a" || arg == "--all" || arg == "-r" || arg == "--remotes" {
            has_list_flag = true;
            i += 1;
            continue;
        }

        if allowed.contains(&arg) {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        // positional argument (branch name)
        has_positional = true;
        i += 1;
    }

    // Reject if there are positional arguments without --list
    if has_positional && !has_list_flag {
        return Err(Rejection {
            reason: "branch creation not allowed".to_string(),
        });
    }

    Ok(())
}

fn validate_remote_args(args: &[&str]) -> Result<(), Rejection> {
    if args.is_empty() {
        return Ok(());
    }

    match args[0] {
        "-v" => {
            // remote -v
            if args.len() > 1 {
                return Err(Rejection {
                    reason: "extra arguments after -v".to_string(),
                });
            }
            Ok(())
        }
        "get-url" => {
            // remote get-url <name>
            if args.len() < 2 {
                return Err(Rejection {
                    reason: "get-url requires a name argument".to_string(),
                });
            }
            Ok(())
        }
        "add" => {
            // remote add <name> <url>
            if args.len() < 3 {
                return Err(Rejection {
                    reason: "add requires name and url arguments".to_string(),
                });
            }
            Ok(())
        }
        "set-url" => {
            // remote set-url <name> <url>
            if args.len() < 3 {
                return Err(Rejection {
                    reason: "set-url requires name and url arguments".to_string(),
                });
            }
            Ok(())
        }
        "remove" | "rm" | "rename" | "prune" | "update" => Err(Rejection {
            reason: format!("remote '{}' not allowed", args[0]),
        }),
        _ => Err(Rejection {
            reason: format!("unknown remote subcommand '{}'", args[0]),
        }),
    }
}

fn validate_fetch_args(args: &[&str]) -> Result<(), Rejection> {
    let not_allowed = ["--prune", "-p", "--force", "-f"];

    for arg in args {
        if not_allowed.contains(arg) {
            return Err(Rejection {
                reason: format!("'{}' not allowed", arg),
            });
        }
    }

    let allowed = [
        "-q",
        "--quiet",
        "--no-tags",
        "--tags",
        "--progress",
        "--no-progress",
    ];

    let mut i = 0;
    let mut after_dd = false;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            after_dd = true;
            i += 1;
            continue;
        }

        if !after_dd && allowed.contains(&arg) {
            i += 1;
            continue;
        }

        if !after_dd && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        // positional (remote/refspec)
        if arg.starts_with('+') {
            return Err(Rejection {
                reason: "refspec starting with '+' not allowed".to_string(),
            });
        }
        if arg.contains(':') {
            return Err(Rejection {
                reason: "refspec with ':' not allowed".to_string(),
            });
        }

        i += 1;
    }

    Ok(())
}

fn validate_push_args(args: &[&str]) -> Result<(), Rejection> {
    let not_allowed = [
        "--force",
        "-f",
        "--force-with-lease",
        "--delete",
        "-d",
        "--mirror",
        "--prune",
        "--all",
    ];

    for arg in args {
        if not_allowed.contains(arg)
            || arg.starts_with("--force-with-lease")
            || arg.starts_with("--forc")
        {
            return Err(Rejection {
                reason: format!("'{}' not allowed", arg),
            });
        }

        // Check for combined flags like -fu
        if arg.starts_with("-f") && arg.len() > 2 {
            return Err(Rejection {
                reason: format!("combined flag '{}' not allowed", arg),
            });
        }
    }

    let allowed = [
        "-u",
        "--set-upstream",
        "-q",
        "--quiet",
        "--porcelain",
        "--no-verify",
        "-n",
        "--dry-run",
        "--atomic",
        "--progress",
        "--no-progress",
        "--tags",
    ];

    let mut i = 0;
    let mut after_dd = false;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            after_dd = true;
            i += 1;
            continue;
        }

        if !after_dd && allowed.contains(&arg) {
            i += 1;
            continue;
        }

        if !after_dd && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        // refspec
        if arg.starts_with('+') || arg.starts_with(':') || arg.contains(":+") {
            return Err(Rejection {
                reason: "dangerous refspec not allowed".to_string(),
            });
        }

        i += 1;
    }

    Ok(())
}

fn validate_merge_args(args: &[&str]) -> Result<(), Rejection> {
    let not_allowed = ["-s", "--strategy", "-X"];

    for arg in args {
        if not_allowed.contains(arg) || arg.starts_with("-s") || arg.starts_with("-X") {
            return Err(Rejection {
                reason: format!("'{}' not allowed", arg),
            });
        }
    }

    let allowed = [
        "--no-edit",
        "--no-ff",
        "--ff",
        "--ff-only",
        "--no-commit",
        "-m",
        "-q",
        "--quiet",
        "--abort",
        "--no-verify",
        "--no-stat",
        "--stat",
    ];

    let mut i = 0;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg == "-m" {
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: "'-m' requires a value".to_string(),
                });
            }
            i += 2;
            continue;
        }

        if allowed.contains(&arg) {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        i += 1;
    }

    Ok(())
}

fn validate_checkout_args(args: &[&str]) -> Result<(), Rejection> {
    let not_allowed = [
        "-f",
        "--force",
        "-B",
        "--orphan",
        "-b",
        "--merge",
        "-m",
        "--overwrite-ignore",
    ];

    for arg in args {
        if not_allowed.contains(arg) {
            return Err(Rejection {
                reason: format!("'{}' not allowed", arg),
            });
        }
    }

    let allowed = ["--ours", "--theirs", "-q", "--quiet", "--detach"];

    for arg in args {
        if *arg == "--" {
            break;
        }

        if !allowed.contains(arg) && arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }
    }

    Ok(())
}

fn validate_restore_args(args: &[&str]) -> Result<(), Rejection> {
    // --source or -s is mandatory
    let mut has_source = false;

    let allowed = [
        "--source",
        "-s",
        "--staged",
        "-S",
        "--worktree",
        "-W",
        "-q",
        "--quiet",
        "--ignore-unmerged",
    ];

    let mut i = 0;
    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            break;
        }

        if arg.starts_with("--source=") || (arg.starts_with("-s") && arg.len() > 2) {
            has_source = true;
            i += 1;
            continue;
        }

        if arg == "--source" || arg == "-s" {
            has_source = true;
            if i + 1 >= args.len() {
                return Err(Rejection {
                    reason: format!("'{}' requires a value", arg),
                });
            }
            i += 2;
            continue;
        }

        if allowed.contains(&arg) {
            i += 1;
            continue;
        }

        if arg.starts_with('-') {
            return Err(Rejection {
                reason: format!("flag '{}' not allowed", arg),
            });
        }

        i += 1;
    }

    if !has_source {
        return Err(Rejection {
            reason: "--source is required".to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_push_force() {
        assert!(validate(&["push", "--force"]).is_err());
        assert!(validate(&["push", "-f"]).is_err());
    }

    #[test]
    fn test_allow_status() {
        assert!(validate(&["status", "--porcelain=v2", "-z", "--branch"]).is_ok());
    }

    #[test]
    fn test_update_ref_d_inside_namespace() {
        assert!(validate(&[
            "update-ref",
            "-d",
            "refs/hikae/snapshots/main/20260101T000000Z"
        ])
        .is_ok());
    }
}
