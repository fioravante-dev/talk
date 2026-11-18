//! Grava o commit do código no binário: duas máquinas com a mesma versão no
//! Cargo.toml podem rodar códigos diferentes, e o `talk-rs --version` mostra qual.
//! Sem git (uma cópia por rsync), vale o `TALK_COMMIT` do ambiente, ou nada.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    for f in ["../../.git/HEAD", "../../.git/index", "../../.git/refs/heads"] {
        println!("cargo:rerun-if-changed={f}");
    }
    println!("cargo:rerun-if-env-changed=TALK_COMMIT");
    let commit = std::env::var("TALK_COMMIT").ok().filter(|c| !c.is_empty()).or_else(|| {
        let hash = git(&["rev-parse", "--short=7", "HEAD"])?;
        let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
        Some(if dirty { format!("{hash}, modificado") } else { hash })
    });
    println!("cargo:rustc-env=TALK_COMMIT={}", commit.unwrap_or_default());
}
