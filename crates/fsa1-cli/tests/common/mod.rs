// Concern: builds a throwaway workbook and spawns the built binary against it | Non-concern: what any verb must print | IO: (argv, cwd) -> exit code + stdout + stderr

// Each test binary including this module uses a different subset of the harness.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// A unique temp directory for one test's workbook, removed by [`Fixture`]'s drop.
pub struct Fixture {
    root: PathBuf,
}

impl Fixture {
    pub fn new(tag: &str) -> Fixture {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("fsa1-cli-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create fixture root");
        Fixture { root }
    }

    pub fn file(&self, tab: &str, name: &str, body: &str) -> &Fixture {
        let dir = self.root.join(tab);
        std::fs::create_dir_all(&dir).expect("create tab dir");
        std::fs::write(dir.join(name), body).expect("write file");
        self
    }

    pub fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

pub fn run(args: &[&str]) -> (i32, String) {
    let (code, stdout, _stderr) = run_err(args);
    (code, stdout)
}

pub fn run_err(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fsa1-cli"))
        .args(args)
        .output()
        .expect("spawn fsa1-cli");
    let code = out.status.code().expect("exit code");
    (
        code,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

pub fn at(fx: &Fixture, rel: &str) -> String {
    fx.path().join(rel).to_str().unwrap().to_string()
}

pub fn run_in(cwd: &Path, args: &[&str]) -> (i32, String) {
    let (code, stdout, _stderr) = run_err_in(cwd, args);
    (code, stdout)
}

pub fn run_err_in(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fsa1-cli"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("spawn fsa1-cli");
    (
        out.status.code().expect("exit code"),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Nothing is excluded — `.cache/` included — so a derived write anywhere under the workbook shows.
pub fn snapshot(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap())
            .collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let path = e.path();
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let ft = e.file_type().unwrap();
            if ft.is_symlink() {
                let target = std::fs::read_link(&path).unwrap();
                out.push(format!("{rel}->{}", target.display()));
            } else if ft.is_dir() {
                walk(&path, base, out);
            } else {
                out.push(format!("{rel}={}", std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}
