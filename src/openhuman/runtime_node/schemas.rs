use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::javascript;
use crate::rpc::RpcOutcome;

#[derive(Debug, Deserialize, Default)]
struct ListToolsParams {}

#[derive(Debug, Deserialize)]
struct ExecuteToolParams {
    tool_name: String,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    prefer_markdown: bool,
}

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("javascript_list_tools"),
        schemas("javascript_execute_tool"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("javascript_list_tools"),
            handler: handle_list_tools,
        },
        RegisteredController {
            schema: schemas("javascript_execute_tool"),
            handler: handle_execute_tool,
        },
    ]
}

pub fn schemas(function: &str) -> ControllerSchema {
    match function {
        "javascript_list_tools" => ControllerSchema {
            namespace: "javascript",
            function: "list_tools",
            description: "List the agent-callable tool registry exposed through the JavaScript runtime bridge.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "tools",
                ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                comment: "List of tool metadata objects: name, description, category, permission_level, scope, supports_markdown, parameters.",
                required: true,
            }],
        },
        "javascript_execute_tool" => ControllerSchema {
            namespace: "javascript",
            function: "execute_tool",
            description: "Execute a named tool through the JavaScript runtime bridge and return its MCP-style ToolResult envelope.",
            inputs: vec![
                FieldSchema {
                    name: "tool_name",
                    ty: TypeSchema::String,
                    comment: "Exact tool name returned by javascript.list_tools.",
                    required: true,
                },
                FieldSchema {
                    name: "args",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "Tool argument object. Defaults to an empty object.",
                    required: false,
                },
                FieldSchema {
                    name: "prefer_markdown",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Bool)),
                    comment: "Hint to tools that can return a compact markdown rendering for LLM consumption.",
                    required: false,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "tool_name",
                    ty: TypeSchema::String,
                    comment: "Echo of the executed tool name.",
                    required: true,
                },
                FieldSchema {
                    name: "elapsed_ms",
                    ty: TypeSchema::U64,
                    comment: "Wall-clock execution time in milliseconds.",
                    required: true,
                },
                FieldSchema {
                    name: "result",
                    ty: TypeSchema::Json,
                    comment: "MCP-style ToolResult payload: {content, is_error, markdownFormatted?}.",
                    required: true,
                },
            ],
        },
        _ => ControllerSchema {
            namespace: "javascript",
            function: "unknown",
            description: "Unknown javascript controller.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "error",
                ty: TypeSchema::String,
                comment: "Lookup error details.",
                required: true,
            }],
        },
    }
}

fn handle_list_tools(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let _: ListToolsParams =
            serde_json::from_value(Value::Object(params)).map_err(|error| error.to_string())?;
        let config = config_rpc::load_config_with_timeout().await?;
        let tools = javascript::list_tools(&config)?;
        let payload = json!({ "tools": tools });
        let log = vec![format!(
            "javascript.list_tools: count={}",
            payload["tools"]
                .as_array()
                .map(|tools| tools.len())
                .unwrap_or(0)
        )];
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

fn handle_execute_tool(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let params: ExecuteToolParams =
            serde_json::from_value(Value::Object(params)).map_err(|error| error.to_string())?;
        let config = config_rpc::load_config_with_timeout().await?;
        let args = if params.args.is_null() {
            json!({})
        } else {
            params.args
        };
        let outcome =
            javascript::execute_tool(&config, &params.tool_name, args, params.prefer_markdown)
                .await?;
        let payload = json!({
            "tool_name": outcome.tool_name,
            "elapsed_ms": outcome.elapsed_ms,
            "result": outcome.result,
        });
        let log = vec![format!(
            "javascript.execute_tool: tool_name={} elapsed_ms={}",
            payload["tool_name"].as_str().unwrap_or("<unknown>"),
            payload["elapsed_ms"].as_u64().unwrap_or(0)
        )];
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_both_runtime_controllers() {
        let schemas = all_controller_schemas();
        assert_eq!(schemas.len(), 2);
        let names: Vec<&str> = schemas.iter().map(|schema| schema.function).collect();
        assert!(names.contains(&"list_tools"));
        assert!(names.contains(&"execute_tool"));
    }

    #[test]
    fn execute_tool_schema_requires_tool_name() {
        let schema = schemas("javascript_execute_tool");
        assert_eq!(schema.namespace, "javascript");
        assert_eq!(schema.function, "execute_tool");
        assert!(schema
            .inputs
            .iter()
            .any(|field| field.name == "tool_name" && field.required));
    }
}
