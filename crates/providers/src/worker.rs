use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use crate::ProviderError;

/// A provider implemented as an external process: one JSON request on stdin,
/// one JSON reply on stdout, then exit.
#[derive(Clone, Debug)]
pub struct ExternalWorker {
    program: PathBuf,
    args: Vec<String>,
    timeout: Duration,
}

#[derive(serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Reply<T> {
    Ok { result: T },
    Error { kind: String, message: String },
}

impl ExternalWorker {
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>, timeout: Duration) -> Self {
        Self {
            program: program.into(),
            args,
            timeout,
        }
    }

    pub async fn call<Req: Serialize, Res: DeserializeOwned>(
        &self,
        request: &Req,
    ) -> Result<Res, ProviderError> {
        let input = serde_json::to_vec(request)
            .map_err(|e| ProviderError::Parse(format!("cannot encode request: {e}")))?;
        let output = tokio::time::timeout(self.timeout, self.run(&input))
            .await
            .map_err(|_| {
                ProviderError::Network(format!("worker timed out after {:?}", self.timeout))
            })??;
        decode_reply(&output)
    }

    async fn run(&self, input: &[u8]) -> Result<Vec<u8>, ProviderError> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                ProviderError::Network(format!("cannot start {}: {e}", self.program.display()))
            })?;
        let io_error = |e: std::io::Error| ProviderError::Network(format!("worker I/O: {e}"));
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input).await.map_err(io_error)?;
        }
        let mut stdout = Vec::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_end(&mut stdout).await.map_err(io_error)?;
        }
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_string(&mut stderr).await.map_err(io_error)?;
        }
        let status = child.wait().await.map_err(io_error)?;
        if !status.success() {
            let tail: String = stderr.lines().rev().take(3).collect::<Vec<_>>().join(" | ");
            return Err(ProviderError::Network(format!(
                "worker exited with {status}: {tail}"
            )));
        }
        Ok(stdout)
    }
}

pub(crate) fn decode_reply<T: DeserializeOwned>(raw: &[u8]) -> Result<T, ProviderError> {
    let reply: Reply<T> = serde_json::from_slice(raw)
        .map_err(|e| ProviderError::Parse(format!("invalid worker reply: {e}")))?;
    match reply {
        Reply::Ok { result } => Ok(result),
        Reply::Error { kind, message } => Err(match kind.as_str() {
            "blocked" => ProviderError::Blocked,
            "rate_limited" => ProviderError::RateLimited,
            "parse" => ProviderError::Parse(message),
            _ => ProviderError::Network(message),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_worker_error_kinds() {
        let reply = br#"{"status":"error","kind":"blocked","message":"consent page"}"#;
        assert_eq!(decode_reply::<()>(reply), Err(ProviderError::Blocked));
        let reply = br#"{"status":"error","kind":"parse","message":"x"}"#;
        assert_eq!(
            decode_reply::<()>(reply),
            Err(ProviderError::Parse("x".into()))
        );
        assert!(matches!(
            decode_reply::<()>(b"garbage"),
            Err(ProviderError::Parse(_))
        ));
    }

    #[tokio::test]
    async fn reports_missing_program_as_network_error() {
        let worker = ExternalWorker::new("/nonexistent/python", Vec::new(), Duration::from_secs(1));
        let result: Result<(), _> = worker.call(&()).await;
        assert!(matches!(result, Err(ProviderError::Network(_))));
    }
}
