use std::{sync::OnceLock, time::Duration};

use reqwest::{Client, RequestBuilder, Response, StatusCode};

use super::{SyncBackendKind, SyncFailure, SyncOperationResult};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ATTEMPTS: usize = 3;

pub(super) fn http_client(backend: Option<SyncBackendKind>) -> SyncOperationResult<Client> {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }

    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| SyncFailure::other(backend, format!("build sync HTTP client: {error}")))?;
    let _ = CLIENT.set(client.clone());
    Ok(client)
}

pub(super) async fn send_with_retry(
    request: RequestBuilder,
    backend: SyncBackendKind,
    operation: &'static str,
) -> SyncOperationResult<Response> {
    for attempt in 0..MAX_ATTEMPTS {
        let Some(attempt_request) = request.try_clone() else {
            return Err(SyncFailure::other(
                Some(backend),
                format!("{operation}: request body cannot be retried"),
            ));
        };
        match attempt_request.send().await {
            Ok(response)
                if attempt + 1 < MAX_ATTEMPTS && should_retry_status(response.status()) =>
            {
                tokio::time::sleep(retry_delay(attempt)).await;
            }
            Ok(response) => return Ok(response),
            Err(error) if attempt + 1 < MAX_ATTEMPTS && is_retryable_error(&error) => {
                tokio::time::sleep(retry_delay(attempt)).await;
            }
            Err(error) => {
                return Err(SyncFailure::other(
                    Some(backend),
                    format!("{operation}: {error}"),
                ));
            }
        }
    }

    Err(SyncFailure::other(
        Some(backend),
        format!("{operation}: retry budget exhausted"),
    ))
}

fn is_retryable_error(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout()
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(150 * (1_u64 << attempt.min(3)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_only_transient_http_statuses() {
        assert!(should_retry_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(should_retry_status(StatusCode::BAD_GATEWAY));
        assert!(!should_retry_status(StatusCode::UNAUTHORIZED));
        assert!(!should_retry_status(StatusCode::CONFLICT));
    }

    #[test]
    fn retry_delay_uses_bounded_exponential_backoff() {
        assert_eq!(retry_delay(0), Duration::from_millis(150));
        assert_eq!(retry_delay(1), Duration::from_millis(300));
        assert_eq!(retry_delay(8), Duration::from_millis(1_200));
    }
}
