use std::{path::Path, process::Stdio};

pub struct PtySession {
    pub child: tokio::process::Child,
}

impl PtySession {
    pub async fn spawn(program: &str, args: &[&str], workspace_dir: &Path) -> anyhow::Result<Self> {
        let mut command = tokio::process::Command::new(program);
        command
            .args(args)
            .current_dir(workspace_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn()?;
        Ok(Self { child })
    }

    pub async fn wait(mut self) -> anyhow::Result<std::process::ExitStatus> {
        Ok(self.child.wait().await?)
    }
}
