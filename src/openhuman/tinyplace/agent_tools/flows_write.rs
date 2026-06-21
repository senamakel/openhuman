//! Write tiny.place flows: register, follow/unfollow, join/create group,
//! post_bounty, submit_work, submissions, job_apply.
//!
//! All declare `Write` + external effect, so the agent harness routes them
//! through the `ApprovalGate`. Identity is always taken from the wallet signer
//! (never an argument) — the agent can only ever act as itself. Paid actions
//! (register, post_bounty) surface an x402 `402` as a **Payment required**
//! fund-and-retry block rather than failing opaquely.

use serde_json::{json, Value};

use tinyplace::types::{
    BountyCreateRequest, BountySubmissionCreateRequest, GroupCreateRequest, ProposalCreateRequest,
};

use crate::openhuman::tools::traits::{Tool, ToolResult};

use super::common::{
    agent_id, call_controller, client, collect_field, err_md, finish, ok_md, opt_bool, opt_str,
    opt_str_list, req_str, resolve_agent, sdk_error, FlowFuture, FlowTool,
};
use super::render::{render_json, Markdown};
use super::suggest::Suggestion;

pub fn write_tools() -> Vec<Box<dyn Tool>> {
    vec![
        FlowTool::write(
            "tinyplace_register",
            "Claim a @handle on tiny.place. Free handles register immediately and \
             publish your discoverable Agent Card. Paid handles preview the on-chain \
             fee first; pass confirm=true to settle the payment from your wallet and \
             claim. Your cryptoId/public key are taken from your wallet automatically.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "handle": { "type": "string", "description": "The @handle to claim (without the @)." },
                    "confirm": { "type": "boolean", "description": "Set true to settle the on-chain fee and claim a paid handle (default false = preview the fee)." }
                },
                "required": ["handle"]
            }),
            register_flow,
        )
        .boxed(),
        FlowTool::write(
            "tinyplace_follow",
            "Follow an agent (by @handle or agentId) so their posts reach your home feed.",
            target_schema("Agent to follow (@handle or agentId)."),
            follow_flow,
        )
        .boxed(),
        FlowTool::write(
            "tinyplace_unfollow",
            "Stop following an agent (by @handle or agentId).",
            target_schema("Agent to unfollow (@handle or agentId)."),
            unfollow_flow,
        )
        .boxed(),
        FlowTool::write(
            "tinyplace_join_group",
            "Join a group by id. Open groups admit you immediately; others queue for \
             approval.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": { "group_id": { "type": "string", "description": "The group id to join." } },
                "required": ["group_id"]
            }),
            join_group_flow,
        )
        .boxed(),
        FlowTool::write(
            "tinyplace_create_group",
            "Create a group you own. Defaults to an open (publicly discoverable) policy.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "name": { "type": "string", "description": "Group name." },
                    "description": { "type": "string", "description": "Optional description." },
                    "policy": { "type": "string", "description": "Membership policy: open | approval | invite-only." },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Optional tags." }
                },
                "required": ["name"]
            }),
            create_group_flow,
        )
        .boxed(),
        FlowTool::write(
            "tinyplace_post_bounty",
            "Create + fund a bounty (contest-style paid work). The reward is escrowed \
             at creation via x402 (SPL only — USDC/CASH). If unfunded it returns a \
             Payment required block; fund, then retry.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "title": { "type": "string", "description": "Bounty title." },
                    "description": { "type": "string", "description": "What the work is." },
                    "amount": { "type": "string", "description": "Reward amount, e.g. '10'." },
                    "asset": { "type": "string", "description": "Reward asset: USDC or CASH (default USDC)." },
                    "days": { "type": "integer", "description": "Days until the deadline." },
                    "deadline": { "type": "string", "description": "RFC3339 deadline (overrides days)." }
                },
                "required": ["title", "amount"]
            }),
            post_bounty_flow,
        )
        .boxed(),
        FlowTool::write(
            "tinyplace_submit_work",
            "Submit your work (a URL) to a bounty. Submitting is free. The submitter \
             is your own identity.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "bounty_id": { "type": "string", "description": "The bounty id." },
                    "url": { "type": "string", "description": "URL of your work." },
                    "title": { "type": "string", "description": "Optional submission title." },
                    "note": { "type": "string", "description": "Optional note to the judges." }
                },
                "required": ["bounty_id", "url"]
            }),
            submit_work_flow,
        )
        .boxed(),
        FlowTool::write(
            "tinyplace_submissions",
            "Review the submissions on a bounty you created, with a council command to \
             trigger judging.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "bounty_id": { "type": "string", "description": "The bounty id." },
                    "limit": { "type": "integer", "description": "Max submissions (default 20)." }
                },
                "required": ["bounty_id"]
            }),
            submissions_flow,
        )
        .boxed(),
        FlowTool::write(
            "tinyplace_job_apply",
            "Submit a proposal (apply) to an open tiny.place job. Free. Your candidate \
             identity is taken from your wallet — it cannot be supplied as an argument.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "job_id": { "type": "string", "description": "The job id to apply for." },
                    "cover_letter": { "type": "string", "description": "Cover letter." },
                    "bid_amount": { "type": "string", "description": "Bid, e.g. '450 USDC'." },
                    "estimated_delivery": { "type": "string", "description": "e.g. '2 weeks'." },
                    "past_work": { "type": "array", "items": { "type": "string" }, "description": "Past work URLs." }
                },
                "required": ["job_id"]
            }),
            job_apply_flow,
        )
        .boxed(),
    ]
}

fn target_schema(desc: &str) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": { "target": { "type": "string", "description": desc } },
        "required": ["target"]
    })
}

fn register_flow(args: Value) -> FlowFuture {
    Box::pin(async move {
        let handle = req_str(&args, "handle")?;
        let handle = handle.trim_start_matches('@').to_string();
        let confirm = opt_bool(&args, "confirm").unwrap_or(false);

        // Route through the `registry_register` controller rather than calling
        // `client.registry.register` directly: the controller fills the
        // signer-derived fields, settles the x402 payment on confirm (retrying
        // while it confirms on-chain), and publishes the directory Agent Card on
        // success — none of which the raw SDK call does.
        let mut params = serde_json::Map::new();
        params.insert("username".to_string(), json!(handle));
        params.insert("confirmed".to_string(), json!(confirm));

        match call_controller("registry_register", params).await {
            Ok(value) => render_register_result(&handle, value),
            Err(message) => {
                let mut md = Markdown::new();
                md.heading(format!("Could not claim @{handle}"));
                md.kv([("Reason", message)]);
                Ok(err_md(md.build()))
            }
        }
    })
}

/// Render the `registry_register` controller result. It returns one of:
/// `{ identity }` (claimed — card published), or `{ challenge, walletBalance,
/// walletAddress }` (paid handle previewed, nothing spent).
fn render_register_result(handle: &str, value: Value) -> anyhow::Result<ToolResult> {
    if let Some(identity) = value.get("identity") {
        let mut md = Markdown::new();
        md.heading(format!("Claimed @{handle}"));
        md.paragraph("Registered and your discoverable Agent Card was published.");
        md.raw_section(render_json(identity));
        return finish(
            md,
            &[
                Suggestion::new("Confirm your identity", "tinyplace_whoami", json!({})),
                Suggestion::new("Start your status loop", "tinyplace_status", json!({})),
            ],
        );
    }
    if let Some(challenge) = value.get("challenge") {
        let mut md = Markdown::new();
        md.heading(format!("@{handle} needs an on-chain fee"));
        md.paragraph(
            "Claiming this handle is a paid action. Review the fee below, ensure your \
             wallet is funded, then re-run with confirm=true to settle and claim.",
        );
        md.subheading("Fee");
        md.raw_section(render_json(challenge));
        if let Some(balance) = value.get("walletBalance") {
            md.kv([("Wallet balance", super::render::scalar(balance))]);
        }
        if let Some(address) = value.get("walletAddress") {
            md.kv([("Wallet address", super::render::scalar(address))]);
        }
        return finish(
            md,
            &[Suggestion::new(
                format!("Settle the fee and claim @{handle}"),
                "tinyplace_register",
                json!({ "handle": handle, "confirm": true }),
            )],
        );
    }
    // Unexpected shape — render whatever came back.
    let mut md = Markdown::new();
    md.heading(format!("Register @{handle}"));
    md.raw_section(render_json(&value));
    Ok(ok_md(md.build()))
}

fn follow_flow(args: Value) -> FlowFuture {
    Box::pin(async move {
        let target = req_str(&args, "target")?;
        let client = client().await?;
        let id = resolve_agent(client, &target).await;
        match client.follows.follow(&id).await {
            Ok(follow) => {
                let v = serde_json::to_value(follow).unwrap_or(Value::Null);
                let mut md = Markdown::new();
                md.heading(format!("Following {target}"));
                md.raw_section(render_json(&v));
                finish(
                    md,
                    &[
                        Suggestion::new("Read your feed", "tinyplace_feed", json!({})),
                        Suggestion::new(
                            format!("Stop following {target}"),
                            "tinyplace_unfollow",
                            json!({ "target": target }),
                        ),
                    ],
                )
            }
            Err(e) => Ok(sdk_error(&format!("Following {target}"), e)),
        }
    })
}

fn unfollow_flow(args: Value) -> FlowFuture {
    Box::pin(async move {
        let target = req_str(&args, "target")?;
        let client = client().await?;
        let id = resolve_agent(client, &target).await;
        match client.follows.unfollow(&id).await {
            Ok(()) => {
                let mut md = Markdown::new();
                md.heading(format!("Unfollowed {target}"));
                finish(md, &[])
            }
            Err(e) => Ok(sdk_error(&format!("Unfollowing {target}"), e)),
        }
    })
}

fn join_group_flow(args: Value) -> FlowFuture {
    Box::pin(async move {
        let group_id = req_str(&args, "group_id")?;
        let client = client().await?;
        // `None` request → the SDK authenticates the join as the wallet signer.
        match client.groups.join(&group_id, None).await {
            Ok(member) => {
                let v = serde_json::to_value(member).unwrap_or(Value::Null);
                let mut md = Markdown::new();
                md.heading(format!("Joined group {group_id}"));
                md.raw_section(render_json(&v));
                finish(
                    md,
                    &[Suggestion::new(
                        format!("See who else is in {group_id}"),
                        "tinyplace_call",
                        json!({ "command": "groups_list", "params": {} }),
                    )],
                )
            }
            Err(e) => Ok(sdk_error(&format!("Joining {group_id}"), e)),
        }
    })
}

fn create_group_flow(args: Value) -> FlowFuture {
    Box::pin(async move {
        let name = req_str(&args, "name")?;
        let client = client().await?;
        // Build via JSON so the membership-policy enum and camelCase wire format
        // are handled by serde rather than re-declared here.
        let mut body = json!({ "name": name });
        if let Some(desc) = opt_str(&args, "description") {
            body["description"] = json!(desc);
        }
        if let Some(policy) = opt_str(&args, "policy") {
            body["membershipPolicy"] = json!(policy);
        }
        if let Some(tags) = opt_str_list(&args, "tags") {
            body["tags"] = json!(tags);
        }
        let request: GroupCreateRequest = serde_json::from_value(body)
            .map_err(|e| anyhow::anyhow!("invalid group params: {e}"))?;
        match client.groups.create(request).await {
            Ok(group) => {
                let v = serde_json::to_value(group).unwrap_or(Value::Null);
                let group_id = collect_field(&v, "groupId").into_iter().next();
                let mut md = Markdown::new();
                md.heading(format!("Created group \"{name}\""));
                md.raw_section(render_json(&v));
                let suggestions = group_id
                    .map(|id| {
                        vec![Suggestion::new(
                            "Create an invite link",
                            "tinyplace_call",
                            json!({ "command": "groups_create_invite", "params": { "groupId": id } }),
                        )]
                    })
                    .unwrap_or_default();
                finish(md, &suggestions)
            }
            Err(e) => Ok(sdk_error(&format!("Creating group \"{name}\""), e)),
        }
    })
}

fn post_bounty_flow(args: Value) -> FlowFuture {
    Box::pin(async move {
        let title = req_str(&args, "title")?;
        let amount = req_str(&args, "amount")?;
        let client = client().await?;
        let mut body = json!({
            "title": title,
            "amount": amount,
            "asset": opt_str(&args, "asset").unwrap_or_else(|| "USDC".to_string()),
        });
        body["description"] = json!(opt_str(&args, "description").unwrap_or_default());
        if let Some(days) = super::common::opt_i64(&args, "days") {
            body["durationDays"] = json!(days);
        }
        if let Some(deadline) = opt_str(&args, "deadline") {
            body["deadline"] = json!(deadline);
        }
        let request: BountyCreateRequest = serde_json::from_value(body).map_err(|e| {
            anyhow::anyhow!("invalid bounty params (check asset is USDC/CASH): {e}")
        })?;
        match client.bounties.create(&request).await {
            Ok(bounty) => {
                let v = serde_json::to_value(bounty).unwrap_or(Value::Null);
                let bounty_id = collect_field(&v, "bountyId").into_iter().next();
                let mut md = Markdown::new();
                md.heading(format!("Posted bounty \"{title}\""));
                md.raw_section(render_json(&v));
                let suggestions = bounty_id
                    .map(|id| {
                        vec![Suggestion::new(
                            "Watch submissions arrive",
                            "tinyplace_submissions",
                            json!({ "bounty_id": id }),
                        )]
                    })
                    .unwrap_or_default();
                finish(md, &suggestions)
            }
            Err(e) => Ok(sdk_error(&format!("Posting bounty \"{title}\""), e)),
        }
    })
}

fn submit_work_flow(args: Value) -> FlowFuture {
    Box::pin(async move {
        let bounty_id = req_str(&args, "bounty_id")?;
        let url = req_str(&args, "url")?;
        let client = client().await?;
        let me = agent_id(client)?;
        let request = BountySubmissionCreateRequest {
            submitter: Some(me),
            submitter_crypto_id: None,
            url,
            title: opt_str(&args, "title"),
            note: opt_str(&args, "note"),
        };
        match client.bounties.submit(&bounty_id, &request).await {
            Ok(submission) => {
                let v = serde_json::to_value(submission).unwrap_or(Value::Null);
                let mut md = Markdown::new();
                md.heading(format!("Submitted to bounty {bounty_id}"));
                md.raw_section(render_json(&v));
                finish(
                    md,
                    &[Suggestion::new(
                        format!("Watch {bounty_id} for the council's decision"),
                        "tinyplace_graphql",
                        json!({ "query": "bounty", "id": bounty_id }),
                    )],
                )
            }
            Err(e) => Ok(sdk_error(&format!("Submitting to {bounty_id}"), e)),
        }
    })
}

fn submissions_flow(args: Value) -> FlowFuture {
    Box::pin(async move {
        let bounty_id = req_str(&args, "bounty_id")?;
        let client = client().await?;
        match client.bounties.list_submissions(&bounty_id, None).await {
            Ok(submissions) => {
                let v = serde_json::to_value(submissions).unwrap_or(Value::Null);
                let mut md = Markdown::new();
                md.heading(format!("Submissions for {bounty_id}"));
                md.raw_section(render_json(&v));
                finish(
                    md,
                    &[Suggestion::new(
                        "Run the judging council now (creator/admin)",
                        "tinyplace_call",
                        json!({ "command": "bounties_run_council", "params": { "bountyId": bounty_id } }),
                    )],
                )
            }
            Err(e) => Ok(sdk_error(
                &format!("Reading submissions for {bounty_id}"),
                e,
            )),
        }
    })
}

fn job_apply_flow(args: Value) -> FlowFuture {
    Box::pin(async move {
        let job_id = req_str(&args, "job_id")?;
        let client = client().await?;
        let me = agent_id(client)?;
        let request = ProposalCreateRequest {
            candidate: me,
            cover_letter: opt_str(&args, "cover_letter"),
            bid_amount: opt_str(&args, "bid_amount"),
            estimated_delivery: opt_str(&args, "estimated_delivery"),
            past_work: opt_str_list(&args, "past_work"),
        };
        match client.jobs.apply(&job_id, &request).await {
            Ok(proposal) => {
                let v = serde_json::to_value(proposal).unwrap_or(Value::Null);
                let mut md = Markdown::new();
                md.heading(format!("Applied to job {job_id}"));
                md.raw_section(render_json(&v));
                finish(
                    md,
                    &[Suggestion::new(
                        format!("Track job {job_id}"),
                        "tinyplace_graphql",
                        json!({ "query": "job", "id": job_id }),
                    )],
                )
            }
            Err(e) => Ok(sdk_error(&format!("Applying to {job_id}"), e)),
        }
    })
}
