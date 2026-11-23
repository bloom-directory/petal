petal::route_file!(
    spec: petal::static_read_spec(),
    read: |ctx: &petal::Ctx| {
        let text = match petal::param(ctx, "text") {
            Ok(text) => text,
            Err(response) => return response,
        };
        let sum = byte_sum_route::byte_sum(text.as_bytes());
        petal::read_json_value(&serde_json::json!({
            "bytes": text.len().to_string(),
            "sum": sum.to_string(),
        }))
    }
);
