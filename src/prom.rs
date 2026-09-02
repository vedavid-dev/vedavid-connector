//! The Prometheus HTTP API client. One place that knows about `/api/v1`.

use std::time::{Duration, Instant};

pub struct Upstream {
    pub status: u16,
    pub body: String,
    pub took_ms: i64,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Unreachable(String);

pub struct Prometheus {
    base: String,
    http: reqwest::Client,
}

const DEFAULT_TIMEOUT_MS: i64 = 30_000;

impl Prometheus {
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    async fn get(
        &self,
        path: &str,
        params: &[(&str, String)],
        timeout_ms: i64,
    ) -> Result<Upstream, Unreachable> {
        let budget = if timeout_ms > 0 {
            timeout_ms
        } else {
            DEFAULT_TIMEOUT_MS
        };
        let started = Instant::now();
        let response = self
            .http
            .get(format!("{}/api/v1/{path}", self.base))
            .query(params)
            .timeout(Duration::from_millis(budget as u64))
            .send()
            .await
            .map_err(|e| Unreachable(e.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|e| Unreachable(e.to_string()))?;
        Ok(Upstream {
            status,
            body,
            took_ms: started.elapsed().as_millis() as i64,
        })
    }

    /// Prometheus takes unix seconds; an absent `time` means now.
    pub async fn instant(
        &self,
        query: &str,
        time_ms: i64,
        timeout_ms: i64,
    ) -> Result<Upstream, Unreachable> {
        let mut params = vec![("query", query.to_string())];
        if time_ms > 0 {
            params.push(("time", secs(time_ms)));
        }
        self.get("query", &params, timeout_ms).await
    }

    pub async fn range(
        &self,
        query: &str,
        start_ms: i64,
        end_ms: i64,
        step_secs: i64,
        timeout_ms: i64,
    ) -> Result<Upstream, Unreachable> {
        let params = vec![
            ("query", query.to_string()),
            ("start", secs(start_ms)),
            ("end", secs(end_ms)),
            ("step", step_secs.to_string()),
        ];
        self.get("query_range", &params, timeout_ms).await
    }

    pub async fn labels(
        &self,
        start_ms: i64,
        end_ms: i64,
        matchers: &[String],
    ) -> Result<Upstream, Unreachable> {
        self.get("labels", &window(start_ms, end_ms, matchers), 0)
            .await
    }

    pub async fn label_values(
        &self,
        label: &str,
        start_ms: i64,
        end_ms: i64,
        matchers: &[String],
    ) -> Result<Upstream, Unreachable> {
        let path = format!("label/{}/values", urlencode(label));
        self.get(&path, &window(start_ms, end_ms, matchers), 0)
            .await
    }

    pub async fn series(
        &self,
        matchers: &[String],
        start_ms: i64,
        end_ms: i64,
    ) -> Result<Upstream, Unreachable> {
        self.get("series", &window(start_ms, end_ms, matchers), 0)
            .await
    }
}

fn secs(ms: i64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

fn window(start_ms: i64, end_ms: i64, matchers: &[String]) -> Vec<(&'static str, String)> {
    let mut params = Vec::new();
    if start_ms > 0 {
        params.push(("start", secs(start_ms)));
    }
    if end_ms > 0 {
        params.push(("end", secs(end_ms)));
    }
    for m in matchers {
        params.push(("match[]", m.clone()));
    }
    params
}

/// Only the label-name path segment needs escaping; reqwest handles the query.
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milliseconds_render_as_prometheus_seconds() {
        assert_eq!(secs(1788347462466), "1788347462.466");
        assert_eq!(secs(1000), "1.000");
    }

    #[test]
    fn an_absent_window_is_omitted_rather_than_sent_as_zero() {
        assert!(window(0, 0, &[]).is_empty());
        let p = window(1000, 2000, &["up".into()]);
        assert_eq!(p.len(), 3);
        assert_eq!(p[2], ("match[]", "up".to_string()));
    }

    #[test]
    fn a_label_name_is_escaped_in_the_path() {
        assert_eq!(urlencode("job"), "job");
        assert_eq!(urlencode("a/b"), "a%2Fb");
        assert_eq!(urlencode("../secrets"), "..%2Fsecrets");
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_is_dropped() {
        assert_eq!(Prometheus::new("http://x:9090/").base, "http://x:9090");
    }
}
