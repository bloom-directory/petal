petal::route_file!(
    spec: petal::static_read_spec().caps(&["bloom:sign.status"]),
    read: |ctx: &petal::Ctx| match petal::signing::status(ctx, "fixture", "message-1") {
        Ok(status) => petal::DispatchResponse::Read(format!("{status:?}").into_bytes()),
        Err(error) => petal::DispatchResponse::Error { code: 500, message: error.to_string() },
    }
);
