use prometheus::{
    register_counter_vec_with_registry,
    register_histogram_vec_with_registry,
    register_gauge_with_registry,
    CounterVec, HistogramVec, Gauge, Registry, TextEncoder, Encoder,
};

#[derive(Clone)]
pub struct Metrics {
    pub registry: Registry,
    pub request_count: CounterVec,
    pub request_duration: HistogramVec,
    pub tokens_total: CounterVec,
    pub cache_operations: CounterVec,
    pub errors_total: CounterVec,
    pub requests_in_flight: Gauge,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let request_count = register_counter_vec_with_registry!(
            "ai_gateway_requests_total",
            "Total requests by provider, model, and status",
            &["provider", "model", "status"],
            registry,
        ).unwrap();

        let request_duration = register_histogram_vec_with_registry!(
            "ai_gateway_request_duration_seconds",
            "Request latency in seconds by provider and model",
            &["provider", "model"],
            vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0],
            registry,
        ).unwrap();

        let tokens_total = register_counter_vec_with_registry!(
            "ai_gateway_tokens_total",
            "Tokens used by provider and type (prompt/completion)",
            &["provider", "type"],
            registry,
        ).unwrap();

        let cache_operations = register_counter_vec_with_registry!(
            "ai_gateway_cache_operations_total",
            "Cache operations by type (hit/miss)",
            &["type"],
            registry,
        ).unwrap();

        let errors_total = register_counter_vec_with_registry!(
            "ai_gateway_errors_total",
            "Errors by provider and error type",
            &["provider", "error_type"],
            registry,
        ).unwrap();

        let requests_in_flight = register_gauge_with_registry!(
            "ai_gateway_requests_in_flight",
            "Current number of in-flight requests",
            registry,
        ).unwrap();

        Self {
            registry,
            request_count,
            request_duration,
            tokens_total,
            cache_operations,
            errors_total,
            requests_in_flight,
        }
    }

    pub fn observe_request(&self, provider: &str, model: &str, status: &str, latency_secs: f64) {
        self.request_count
            .with_label_values(&[provider, model, status])
            .inc();
        self.request_duration
            .with_label_values(&[provider, model])
            .observe(latency_secs);
    }

    pub fn record_tokens(&self, provider: &str, prompt: u32, completion: u32) {
        self.tokens_total
            .with_label_values(&[provider, "prompt"])
            .inc_by(prompt as f64);
        self.tokens_total
            .with_label_values(&[provider, "completion"])
            .inc_by(completion as f64);
    }

    pub fn cache_hit(&self) {
        self.cache_operations.with_label_values(&["hit"]).inc();
    }

    pub fn cache_miss(&self) {
        self.cache_operations.with_label_values(&["miss"]).inc();
    }

    pub fn record_error(&self, provider: &str, error_type: &str) {
        self.errors_total
            .with_label_values(&[provider, error_type])
            .inc();
    }

    pub fn encode(&self) -> String {
        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();
        let metric_families = self.registry.gather();
        let _ = encoder.encode(&metric_families, &mut buffer);
        String::from_utf8(buffer).unwrap_or_default()
    }
}
