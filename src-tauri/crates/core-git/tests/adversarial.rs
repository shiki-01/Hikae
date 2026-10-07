//! 許可リストの回避を試みる入力の検査。1 件でも通ったら安全設計が崩れる。

use core_git::validate;

const OID: &str = "0123456789abcdef0123456789abcdef01234567";

fn assert_rejected(args: &[&str]) {
    assert!(
        validate(args).is_err(),
        "拒否されるべき呼び出しが通った: {args:?}"
    );
}

fn assert_allowed(args: &[&str]) {
    assert!(
        validate(args).is_ok(),
        "許可されるべき呼び出しが拒否された: {args:?} -> {:?}",
        validate(args)
    );
}

#[test]
fn push_refspec_after_double_dash_is_checked() {
    assert_rejected(&["push", "origin", "--", "+main"]);
    assert_rejected(&["push", "origin", "--", ":main"]);
    assert_rejected(&["push", "origin", "--", "HEAD:+main"]);
}

#[test]
fn fetch_refspec_after_double_dash_is_checked() {
    assert_rejected(&["fetch", "origin", "--", "+refs/heads/*:refs/heads/*"]);
}

#[test]
fn push_option_abbreviations_are_rejected() {
    for flag in [
        "--forc",
        "--force-w",
        "--force-with-lease=main:abc",
        "--del",
        "--delet",
        "--mirr",
        "--prun",
        "--al",
        "-fu",
        "-uf",
        "-df",
    ] {
        assert_rejected(&["push", flag, "origin", "main"]);
    }
}

#[test]
fn update_ref_is_limited_to_app_namespace() {
    // -d を複数回指定して最後の ref だけ検査させる手口
    assert_rejected(&[
        "update-ref",
        "-d",
        "refs/heads/main",
        "-d",
        "refs/hikae/snapshots/x",
    ]);
    assert_rejected(&[
        "update-ref",
        "-d",
        "refs/hikae/snapshots/x",
        "-d",
        "refs/heads/main",
    ]);
    // 全ゼロ OID による削除
    assert_rejected(&[
        "update-ref",
        "refs/hikae/snapshots/x",
        "0000000000000000000000000000000000000000",
    ]);
    // ブランチや HEAD の付け替え
    assert_rejected(&["update-ref", "refs/heads/main", OID]);
    assert_rejected(&["update-ref", "HEAD", OID]);
    assert_rejected(&["update-ref", "refs/tags/v1", OID]);
    // 名前空間の見かけ上の一致
    assert_rejected(&["update-ref", "refs/hikae-evil/x", OID]);
    assert_rejected(&["update-ref", "refs/hikae/../heads/main", OID]);
    assert_rejected(&["update-ref", "--stdin"]);
    assert_rejected(&["update-ref"]);
    assert_allowed(&["update-ref", "-d", "refs/hikae/snapshots/main/1"]);
    assert_allowed(&[
        "update-ref",
        "-m",
        "snapshot",
        "refs/hikae/snapshots/main/1",
        OID,
    ]);
}

#[test]
fn global_option_variants_are_rejected() {
    for args in [
        &["-C", "/tmp", "status"][..],
        &["--git-dir=x", "status"],
        &["--work-tree=x", "status"],
        &["--exec-path=x", "status"],
        &["-p", "status"],
        &["--paginate", "status"],
        &["--namespace=x", "status"],
        &["-c", "Alias.x=!rm", "status"],
        &["-c", "alias.x=!rm", "status"],
        &["-c", "core.sshcommand=evil", "status"],
        &["-c", "core.editor=evil", "commit"],
        &["-c", "include.path=evil", "status"],
        &["-c", "includeIf.gitdir:/.path=evil", "status"],
        &["-c", "core.hooksPath", "status"],
        &["-ccore.sshCommand=evil", "status"],
        &["--config-env=core.sshCommand=X", "status"],
        &["--version", "status"],
    ] {
        assert_rejected(args);
    }
}

#[test]
fn destructive_subcommands_are_rejected() {
    for args in [
        &["reset"][..],
        &["reset", "--hard"],
        &["reset", "--hard", "HEAD~1"],
        &["reset", "--merge"],
        &["clean"],
        &["clean", "-fdx"],
        &["rebase"],
        &["rebase", "--abort"],
        &["filter-branch"],
        &["filter-repo"],
        &["gc"],
        &["gc", "--prune=now"],
        &["prune"],
        &["reflog"],
        &["reflog", "expire", "--expire=now", "--all"],
        &["stash"],
        &["stash", "drop"],
        &["tag", "-d", "x"],
        &["pull"],
        &["cherry-pick", "x"],
        &["revert", "x"],
        &["worktree", "remove", "x"],
        &["submodule", "update"],
        &["replace", "a", "b"],
        &["bisect", "reset"],
        &["am"],
        &["apply", "x.patch"],
        &["mv", "a", "b"],
        &["fsck"],
        &["repack", "-ad"],
        &["maintenance", "run"],
        &["hash-object", "-w", "x"],
        &["fast-import"],
        &["send-pack"],
        &["receive-pack"],
        &["upload-pack"],
        &["credential", "reject"],
        &["unknown-subcommand"],
        &[""],
        &[],
    ] {
        assert_rejected(args);
    }
}

#[test]
fn checkout_and_restore_variants() {
    assert_rejected(&["checkout", "-f"]);
    assert_rejected(&["checkout", "--force"]);
    assert_rejected(&["checkout", "--forc"]);
    assert_rejected(&["checkout", "-fq", "x"]);
    assert_rejected(&["checkout", "-B", "x"]);
    assert_rejected(&["checkout", "--orphan", "x"]);
    assert_rejected(&["restore", "x"]);
    assert_rejected(&["restore", "--staged", "x"]);
    assert_rejected(&["restore", "--worktree", "x"]);
    assert_rejected(&["restore", "--", "--source=HEAD"]);
    assert_rejected(&["restore", "--sourc=HEAD", "x"]);
    assert_allowed(&["restore", "--source=HEAD", "--", "x"]);
}

#[test]
fn branch_and_remote_variants() {
    for args in [
        &["branch", "-D", "x"][..],
        &["branch", "-d", "x"],
        &["branch", "--delet", "x"],
        &["branch", "-fD", "x"],
        &["branch", "-m", "a", "b"],
        &["branch", "-M", "a", "b"],
        &["branch", "-c", "a", "b"],
        &["branch", "x"],
        &["branch", "--set-upstream-to=x"],
        &["branch", "--unset-upstream"],
        &["remote", "remove", "o"],
        &["remote", "rm", "o"],
        &["remote", "rename", "a", "b"],
        &["remote", "prune", "o"],
        &["remote", "update"],
        &["remote", "set-head", "o", "x"],
        &["remote", "set-branches", "o", "x"],
    ] {
        assert_rejected(args);
    }
    assert_allowed(&["branch", "--show-current"]);
    assert_allowed(&["remote", "get-url", "origin"]);
}

#[test]
fn commit_variants() {
    for args in [
        &["commit", "--amend"][..],
        &["commit", "--amen"],
        &["commit", "--am"],
        &["commit", "-a", "-m", "x"],
        &["commit", "-am", "x"],
        &["commit", "--all", "-m", "x"],
        &["commit", "-C", "HEAD"],
        &["commit", "--fixup=HEAD"],
        &["commit", "--squash=HEAD"],
        &["commit", "--reset-author"],
    ] {
        assert_rejected(args);
    }
}

#[test]
fn rm_and_add_variants() {
    assert_rejected(&["rm", "x"]);
    assert_rejected(&["rm", "-f", "x"]);
    assert_rejected(&["rm", "--cached", "-f", "x"]);
    assert_rejected(&["rm", "--cached", "--force", "x"]);
    assert_rejected(&["rm", "--", "--cached", "x"]);
    assert_rejected(&["add", "-f", "x"]);
    assert_rejected(&["add", "--force", "x"]);
    assert_rejected(&["config", "--local", "--file", "x", "a", "b"]);
    assert_rejected(&["config", "--local", "--global", "a", "b"]);
    assert_rejected(&["config", "--global", "--local", "a", "b"]);
    assert_rejected(&["config", "a", "b"]);
}

#[test]
fn merge_variants() {
    assert_rejected(&["merge", "-s", "ours", "x"]);
    assert_rejected(&["merge", "--strategy=ours", "x"]);
    assert_rejected(&["merge", "-Xtheirs", "x"]);
    assert_rejected(&["merge", "--squash", "x"]);
    assert_allowed(&["merge", "--no-edit", "x"]);
}
