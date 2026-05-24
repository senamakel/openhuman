//! Linux backend: Landlock LSM (kernel 5.13+).
//!
//! Reuses the existing [`crate::openhuman::security::landlock`] implementation
//! but wraps it behind the [`JailBackend`] trait so callers don't have to
//! plumb `SecurityConfig`. The actual ruleset application happens *in the
//! parent process* before `exec`, so the child inherits the restrictions —
//! same model used by Chromium's Linux sandbox.

#![cfg(target_os = "linux")]

use std::process::{Child, Command};

use super::jail::{Jail, JailBackend};

pub struct LandlockBackend;

impl LandlockBackend {
    pub fn new() -> Self {
        Self
    }
}

impl JailBackend for LandlockBackend {
    fn name(&self) -> &'static str {
        "landlock"
    }

    fn is_available(&self) -> bool {
        #[cfg(feature = "sandbox-landlock")]
        {
            use landlock::{AccessFs, Ruleset, RulesetAttr, RulesetCreatedAttr};
            Ruleset::default()
                .handle_access(AccessFs::ReadFile)
                .and_then(|r| r.create())
                .is_ok()
        }
        #[cfg(not(feature = "sandbox-landlock"))]
        {
            false
        }
    }

    fn spawn(&self, jail: &Jail, mut cmd: Command) -> std::io::Result<Child> {
        #[cfg(feature = "sandbox-landlock")]
        {
            use landlock::{
                AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr,
            };
            use std::os::unix::process::CommandExt;

            let root = jail.root.clone();
            let read_only = jail.read_only.clone();

            // SAFETY: pre_exec runs after fork() in the child, before exec.
            // We apply Landlock there so the parent process keeps its
            // privileges (the parent may legitimately need broader access).
            unsafe {
                cmd.pre_exec(move || {
                    let mut ruleset = Ruleset::default()
                        .handle_access(
                            AccessFs::ReadFile
                                | AccessFs::WriteFile
                                | AccessFs::ReadDir
                                | AccessFs::RemoveDir
                                | AccessFs::RemoveFile
                                | AccessFs::MakeReg
                                | AccessFs::MakeDir
                                | AccessFs::MakeSym,
                        )
                        .and_then(|r| r.create())
                        .map_err(|e| std::io::Error::other(e.to_string()))?;

                    let root_fd =
                        PathFd::new(&root).map_err(|e| std::io::Error::other(e.to_string()))?;
                    ruleset = ruleset
                        .add_rule(PathBeneath::new(
                            root_fd,
                            AccessFs::ReadFile
                                | AccessFs::WriteFile
                                | AccessFs::ReadDir
                                | AccessFs::RemoveFile
                                | AccessFs::RemoveDir
                                | AccessFs::MakeReg
                                | AccessFs::MakeDir,
                        ))
                        .map_err(|e| std::io::Error::other(e.to_string()))?;

                    for ro in &read_only {
                        if let Ok(fd) = PathFd::new(ro) {
                            ruleset = ruleset
                                .add_rule(PathBeneath::new(
                                    fd,
                                    AccessFs::ReadFile | AccessFs::ReadDir,
                                ))
                                .map_err(|e| std::io::Error::other(e.to_string()))?;
                        }
                    }

                    ruleset
                        .restrict_self()
                        .map_err(|e| std::io::Error::other(e.to_string()))?;
                    Ok(())
                });
            }

            cmd.spawn()
        }
        #[cfg(not(feature = "sandbox-landlock"))]
        {
            let _ = jail;
            cmd.spawn()
        }
    }
}
