pub(crate) fn classify_http_status(status: u16) -> (bool, bool, String, String) {
    if (200..300).contains(&status) {
        return (
            true,
            true,
            "unknown".to_string(),
            format!("Connected (HTTP {}).", status),
        );
    }

    if matches!(status, 400 | 401 | 403 | 404 | 405 | 429) {
        let message = match status {
            400 => "Endpoint reachable, request rejected (HTTP 400).",
            401 => "Endpoint reachable, authorization failed (HTTP 401).",
            403 => "Endpoint reachable, access denied (HTTP 403).",
            404 => "Endpoint reachable, route not found (HTTP 404).",
            405 => "Endpoint reachable, method not allowed (HTTP 405).",
            429 => "Endpoint reachable, rate limited (HTTP 429).",
            _ => "Endpoint reachable.",
        };
        return (true, true, "unknown".to_string(), message.to_string());
    }

    (
        false,
        true,
        "unknown".to_string(),
        format!("Endpoint reachable but unhealthy (HTTP {}).", status),
    )
}

pub(crate) fn classify_runtime_probe_status(status: u16, capability: &str) -> (bool, bool, String, String) {
    let capability_label = match capability {
        "responses" => "Responses API",
        "chat_completions" => "Chat Completions API",
        _ => "Runtime API",
    };

    if (200..300).contains(&status) {
        return (
            true,
            true,
            "supported".to_string(),
            format!(
                "{} accepted the request (HTTP {}).",
                capability_label, status
            ),
        );
    }

    if matches!(status, 400 | 401 | 403 | 405 | 422 | 429) {
        let message = match status {
            400 => format!(
                "{} is present, but this probe payload was rejected (HTTP 400).",
                capability_label
            ),
            401 => format!(
                "{} is present, but authorization failed (HTTP 401).",
                capability_label
            ),
            403 => format!(
                "{} is present, but access was denied (HTTP 403).",
                capability_label
            ),
            405 => format!(
                "{} route exists, but the HTTP method was rejected (HTTP 405).",
                capability_label
            ),
            422 => format!(
                "{} is present, but the request body was semantically invalid (HTTP 422).",
                capability_label
            ),
            429 => format!(
                "{} is present, but the provider rate limited the probe (HTTP 429).",
                capability_label
            ),
            _ => format!("{} appears to be supported.", capability_label),
        };
        return (false, true, "supported".to_string(), message);
    }

    if status == 404 {
        return (
            false,
            true,
            "unsupported".to_string(),
            format!("{} route was not found (HTTP 404).", capability_label),
        );
    }

    (
        false,
        true,
        "unknown".to_string(),
        format!(
            "{} probe reached the server, but compatibility is unclear (HTTP {}).",
            capability_label, status
        ),
    )
}
