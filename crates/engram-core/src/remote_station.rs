//! Remote rendering station client.
//!
//! Dispatches media generation workloads (image, video, audio) over SSH to a
//! dedicated desktop rendering machine, allowing the local MacBook agent to
//! direct and orchestrate rendering asynchronously without blocking local compute.

use crate::config::RenderStationConfig;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderJobType {
    Image,
    Video,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderJob {
    pub id: String,
    pub job_type: RenderJobType,
    pub prompt: String,
    pub output_filename: String,
    pub params: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_pid: Option<u32>,
    pub status: RenderStatus,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RenderJob {
    pub fn new(
        job_type: RenderJobType,
        prompt: impl Into<String>,
        output_filename: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        let id = format!(
            "job-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Self {
            id,
            job_type,
            prompt: prompt.into(),
            output_filename: output_filename.into(),
            params,
            remote_pid: None,
            status: RenderStatus::Queued,
            created_at: now,
            completed_at: None,
            error: None,
        }
    }
}

pub struct RemoteStationClient {
    pub config: RenderStationConfig,
}

impl RemoteStationClient {
    pub fn new(config: RenderStationConfig) -> Self {
        Self { config }
    }

    pub fn is_configured(&self) -> bool {
        !self.config.host.trim().is_empty()
    }

    pub fn ssh_target(&self) -> String {
        if self.config.user.is_empty() {
            self.config.host.clone()
        } else {
            format!("{}@{}", self.config.user, self.config.host)
        }
    }

    /// Shell-safe escape for strings injected into remote bash commands.
    pub fn escape_shell_arg(arg: &str) -> String {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }

    /// Build the core command that runs on the desktop.
    pub fn build_inner_command(&self, job: &RenderJob) -> String {
        let job_type_str = match job.job_type {
            RenderJobType::Image => "image",
            RenderJobType::Video => "video",
            RenderJobType::Audio => "audio",
        };
        let ws = self.config.remote_workspace.trim_end_matches('/');
        let prompt_esc = Self::escape_shell_arg(&job.prompt);
        let out_path = format!("{}/out/{}", ws, job.output_filename);
        let out_esc = Self::escape_shell_arg(&out_path);
        let params_str = serde_json::to_string(&job.params).unwrap_or_else(|_| "{}".into());
        let params_esc = Self::escape_shell_arg(&params_str);

        if self.config.prefer_pinokio {
            format!(
                "pinokio run {}/station/pinokio.json --type {} --prompt {} --output {} --params {}",
                ws, job_type_str, prompt_esc, out_esc, params_esc
            )
        } else {
            format!(
                "python3 {}/station/runner.py --type {} --prompt {} --output {} --params {}",
                ws, job_type_str, prompt_esc, out_esc, params_esc
            )
        }
    }

    /// Build the detached, non-blocking execution wrapper.
    pub fn build_dispatch_command(&self, job: &RenderJob) -> String {
        let ws = self.config.remote_workspace.trim_end_matches('/');
        let inner = self.build_inner_command(job);
        let log_file = format!("{}/jobs/{}.log", ws, job.id);
        let pid_file = format!("{}/jobs/{}.pid", ws, job.id);
        let status_file = format!("{}/jobs/{}.status", ws, job.id);

        format!(
            "mkdir -p {ws}/jobs {ws}/out && ( {inner} > {log_file} 2>&1 ; echo $? > {status_file} ) & echo $! > {pid_file} && cat {pid_file}",
            ws = ws,
            inner = inner,
            log_file = log_file,
            pid_file = pid_file,
            status_file = status_file,
        )
    }

    /// Construct `ssh` command arguments.
    pub fn build_ssh_args(&self, remote_cmd: &str) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
        ];
        if self.config.port != 22 && self.config.port != 0 {
            args.push("-p".to_string());
            args.push(self.config.port.to_string());
        }
        if let Some(ref key) = self.config.key_path {
            args.push("-i".to_string());
            args.push(key.to_string_lossy().to_string());
        }
        args.push(self.ssh_target());
        args.push(remote_cmd.to_string());
        args
    }

    /// Construct `scp` command arguments for pulling artifacts.
    pub fn build_scp_args(&self, remote_relative_or_abs: &str, local_dest: &Path) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
        ];
        if self.config.port != 22 && self.config.port != 0 {
            args.push("-P".to_string());
            args.push(self.config.port.to_string());
        }
        if let Some(ref key) = self.config.key_path {
            args.push("-i".to_string());
            args.push(key.to_string_lossy().to_string());
        }
        let remote_src = format!("{}:{}", self.ssh_target(), remote_relative_or_abs);
        args.push(remote_src);
        args.push(local_dest.to_string_lossy().to_string());
        args
    }

    /// Dispatch job to remote station asynchronously without blocking.
    pub fn dispatch_job(&self, job: &mut RenderJob) -> Result<()> {
        if !self.is_configured() {
            anyhow::bail!("Remote station host is not configured in settings");
        }
        let remote_cmd = self.build_dispatch_command(job);
        let ssh_args = self.build_ssh_args(&remote_cmd);

        let output = Command::new("ssh")
            .args(&ssh_args)
            .output()
            .with_context(|| format!("failed to invoke ssh to {}", self.ssh_target()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            job.status = RenderStatus::Failed;
            job.error = Some(stderr.trim().to_string());
            anyhow::bail!("remote station dispatch failed: {stderr}");
        }

        let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Ok(pid) = pid_str.parse::<u32>() {
            job.remote_pid = Some(pid);
            job.status = RenderStatus::Running;
        } else {
            job.status = RenderStatus::Running;
        }
        Ok(())
    }

    /// Check status of in-flight remote job.
    pub fn poll_job_status(&self, job: &mut RenderJob) -> Result<RenderStatus> {
        if !self.is_configured() {
            return Ok(job.status);
        }
        let ws = self.config.remote_workspace.trim_end_matches('/');
        let status_file = format!("{}/jobs/{}.status", ws, job.id);
        let pid_probe = if let Some(pid) = job.remote_pid {
            format!("( kill -0 {pid} 2>/dev/null && echo RUNNING || ( [ -f {status_file} ] && cat {status_file} || echo FINISHED ) )")
        } else {
            format!("( [ -f {status_file} ] && cat {status_file} || echo RUNNING )")
        };

        let ssh_args = self.build_ssh_args(&pid_probe);
        let output = Command::new("ssh").args(&ssh_args).output()?;
        let res = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if res == "RUNNING" {
            job.status = RenderStatus::Running;
        } else if res == "0" || res == "FINISHED" {
            job.status = RenderStatus::Completed;
            job.completed_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            );
        } else {
            job.status = RenderStatus::Failed;
            job.error = Some(format!("process exited with code {res}"));
        }
        Ok(job.status)
    }

    /// Fetch finished artifact from remote desktop to local destination directory.
    pub fn fetch_artifact(&self, job: &RenderJob, local_dest_dir: &Path) -> Result<PathBuf> {
        let ws = self.config.remote_workspace.trim_end_matches('/');
        let remote_artifact_path = format!("{}/out/{}", ws, job.output_filename);
        std::fs::create_dir_all(local_dest_dir)?;
        let local_dest = local_dest_dir.join(&job.output_filename);

        let scp_args = self.build_scp_args(&remote_artifact_path, local_dest_dir);
        let output = Command::new("scp")
            .args(&scp_args)
            .output()
            .with_context(|| format!("failed to copy artifact from {}", remote_artifact_path))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("failed to fetch remote artifact: {stderr}");
        }

        Ok(local_dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes_cleanly() {
        assert_eq!(
            RemoteStationClient::escape_shell_arg("a 'quote' test"),
            "'a '\\''quote'\\'' test'"
        );
    }

    #[test]
    fn formats_custom_python_runner_command() {
        let mut cfg = RenderStationConfig::default();
        cfg.host = "192.168.1.50".to_string();
        cfg.user = "renderuser".to_string();
        let client = RemoteStationClient::new(cfg);

        let job = RenderJob::new(
            RenderJobType::Video,
            "cinematic shot of cyber city",
            "shot01.mp4",
            serde_json::json!({"fps": 24, "steps": 30}),
        );

        let inner = client.build_inner_command(&job);
        assert!(inner.contains("python3 ~/.engram-station/station/runner.py --type video"));
        assert!(inner.contains("--prompt 'cinematic shot of cyber city'"));
        assert!(inner.contains("--output '~/.engram-station/out/shot01.mp4'"));
    }

    #[test]
    fn formats_pinokio_runner_command_when_preferred() {
        let mut cfg = RenderStationConfig::default();
        cfg.host = "desktop.local".to_string();
        cfg.prefer_pinokio = true;
        let client = RemoteStationClient::new(cfg);

        let job = RenderJob::new(
            RenderJobType::Image,
            "portrait in sunset",
            "frame.png",
            serde_json::json!({}),
        );

        let inner = client.build_inner_command(&job);
        assert!(inner.contains("pinokio run ~/.engram-station/station/pinokio.json --type image"));
    }

    #[test]
    fn constructs_ssh_arguments_with_port_and_key() {
        let mut cfg = RenderStationConfig::default();
        cfg.host = "10.0.0.12".to_string();
        cfg.user = "art".to_string();
        cfg.port = 2222;
        cfg.key_path = Some(PathBuf::from("/Users/tk/.ssh/id_ed25519"));
        let client = RemoteStationClient::new(cfg);

        let args = client.build_ssh_args("uptime");
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"2222".to_string()));
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"/Users/tk/.ssh/id_ed25519".to_string()));
        assert_eq!(args.last().unwrap(), "uptime");
    }
}
