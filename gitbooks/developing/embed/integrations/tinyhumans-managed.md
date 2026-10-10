---
description: "Install the host backend transport before using managed inference and hosted services."
icon: code
---

# TinyHumans managed inference

Embed wraps the core; it does not contain a TinyHumans SDK backend client. A managed host must supply a `BackendTransport` through `RuntimeBuilder::backend_transport`, or boot through the SDK-backed builder in `openhuman-tinyhumans`. The connected host layer owns transport and session login. `RuntimeBuilder::backend_url` selects a URL; it does not install a transport.

Supply a library backend key with `RuntimeBuilder::api_key` and initialize the process master key before credential storage. Library mode does not perform user login. Managed inference uses the key as a bearer; backend REST calls resolve it through the backend credential seam. Without an installed transport, hosted backend calls return a typed backend-unavailable error.

The SaaS example uses a deterministic implementation of that transport boundary and installs separate profile credentials. It verifies managed turns without a live service.

<!-- BEGIN EMBED: crates/openhuman-embed/examples/profiles.rs#profiles -->

```rust
    let profiles = openhuman_embed::ProfileRuntime::build(config).await?;
    let mut handles = Vec::new();
    for (user, text) in [("alice", "Alice private note"), ("bob", "Bob private note")] {
        let profile = profiles.provision(user).await?;
        profiles
            .set_credential(
                &profile.profile_id,
                openhuman_embed::profiles::ProfileCredentialKind::ApiKey,
                &format!("{user}-fixture"),
            )
            .await?;
        let handle = profiles.open(user).await?;
        let reply = handle.chat("shared-thread", text).await?;
        assert!(reply.text.contains(text), "{}", reply.text);
        handles.push(handle);
    }
    assert_ne!(handles[0].workspace_dir(), handles[1].workspace_dir());
    for (index, own, other) in [
        (0, "Alice private note", "Bob private note"),
        (1, "Bob private note", "Alice private note"),
    ] {
        let messages = handles[index].messages("shared-thread").await?;
        let text = serde_json::to_string(&messages)?;
        assert!(text.contains(own));
        assert!(!text.contains(other));
    }
    drop(handles);
    profiles.shutdown().await;
    println!("two profiles keep same-thread conversations separate");
```

<!-- END EMBED -->

See the [complete profiles example](https://github.com/tinyhumansai/openhuman/blob/main/crates/openhuman-embed/examples/profiles.rs) and its shared support transport for the full offline setup. For a deployed SDK-backed host, follow the connected layer's setup instead of copying the loopback transport.

Use [OpenAI-compatible routing](openai-compatible.md) when your host wants to send inference directly to a BYOK endpoint. That explicit provider route does not supply hosted backend services.
