use crate::{Arcade, CoreError};
use arcade_contract::{ResultStatus, ToolRequest, ToolValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
};
use thiserror::Error;

/// A versioned DAG. Stage outputs are references; file content never enters the
/// pipeline definition or frontend IPC as a large serialized buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pipeline {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub version: u32,
    pub nodes: Vec<PipelineNode>,
    pub output_nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineNode {
    pub id: String,
    pub tool_id: String,
    pub inputs: Vec<InputSource>,
    #[serde(default)]
    pub options: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputSource {
    External {
        index: usize,
    },
    Node {
        #[serde(rename = "nodeId")]
        node_id: String,
        #[serde(rename = "outputIndex")]
        output_index: usize,
    },
}

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("duplicate stage ID {0}")]
    DuplicateStage(String),
    #[error("pipeline must expose at least one output stage")]
    NoOutputs,
    #[error("unknown stage {0}")]
    UnknownStage(String),
    #[error("unknown tool {0}")]
    UnknownTool(String),
    #[error("stage {0} requires at least one tool input")]
    MissingInputs(String),
    #[error("stage {stage} requires at least {required} tool inputs")]
    TooFewInputs { stage: String, required: usize },
    #[error("cycle in pipeline")]
    Cycle,
    #[error("incompatible connection from {from} ({output_type}) to {to} ({input_types:?})")]
    TypeMismatch {
        from: String,
        to: String,
        output_type: String,
        input_types: Vec<String>,
    },
    #[error("pipeline cancelled")]
    Cancelled,
    #[error("stage {0}: {1}")]
    Execution(String, #[source] CoreError),
    #[error("stage {0} produced no output")]
    MissingOutput(String),
    #[error("pipeline external input {0} is missing")]
    MissingExternal(usize),
    #[error(
        "stage {stage} uses sensitive option {key}; password controls cannot be saved in pipelines"
    )]
    SensitiveOption { stage: String, key: String },
    #[error("stage {0} failed: {1}")]
    StageFailed(String, String),
}

impl Pipeline {
    pub fn validate<'a>(
        &'a self,
        runtime: &Arcade,
    ) -> Result<Vec<&'a PipelineNode>, PipelineError> {
        if self.output_nodes.is_empty() {
            return Err(PipelineError::NoOutputs);
        }
        let tools = runtime.list_tools();
        let mut by_id = HashMap::new();
        for node in &self.nodes {
            if by_id.insert(node.id.as_str(), node).is_some() {
                return Err(PipelineError::DuplicateStage(node.id.clone()));
            }
            if !tools.iter().any(|tool| tool.id == node.tool_id) {
                return Err(PipelineError::UnknownTool(node.tool_id.clone()));
            }
            let tool = tools.iter().find(|tool| tool.id == node.tool_id).unwrap();
            let required_inputs = tool
                .ui
                .as_ref()
                .map(|ui| {
                    ui.input.min_items.unwrap_or(match ui.input.kind {
                        arcade_contract::UiInputKind::None
                        | arcade_contract::UiInputKind::Folder => 0,
                        _ => 1,
                    }) as usize
                })
                .unwrap_or_else(|| usize::from(!tool.inputs.is_empty()));
            if node.inputs.len() < required_inputs {
                return Err(if required_inputs == 1 {
                    PipelineError::MissingInputs(node.id.clone())
                } else {
                    PipelineError::TooFewInputs {
                        stage: node.id.clone(),
                        required: required_inputs,
                    }
                });
            }
            if let Some(controls) = tool.ui.as_ref().map(|ui| &ui.controls) {
                if let Some(key) = controls
                    .iter()
                    .filter(|control| control.kind == arcade_contract::UiControlKind::Password)
                    .map(|control| control.key.as_str())
                    .find(|key| node.options.get(*key).is_some())
                {
                    return Err(PipelineError::SensitiveOption {
                        stage: node.id.clone(),
                        key: key.to_owned(),
                    });
                }
            }
        }
        for id in &self.output_nodes {
            if !by_id.contains_key(id.as_str()) {
                return Err(PipelineError::UnknownStage(id.clone()));
            }
        }
        let mut ordered = Vec::new();
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        for node in &self.nodes {
            visit(node, &by_id, &mut visiting, &mut visited, &mut ordered)?;
        }
        for node in &ordered {
            for input in &node.inputs {
                if let InputSource::Node {
                    node_id,
                    output_index,
                } = input
                {
                    let source = by_id
                        .get(node_id.as_str())
                        .ok_or_else(|| PipelineError::UnknownStage(node_id.clone()))?;
                    let from = tools.iter().find(|tool| tool.id == source.tool_id).unwrap();
                    let to = tools.iter().find(|tool| tool.id == node.tool_id).unwrap();
                    let output_type = from
                        .outputs
                        .get(*output_index)
                        .ok_or_else(|| PipelineError::MissingOutput(source.id.clone()))?;
                    if !to
                        .inputs
                        .iter()
                        .any(|input_type| compatible(output_type, input_type))
                    {
                        return Err(PipelineError::TypeMismatch {
                            from: source.id.clone(),
                            to: node.id.clone(),
                            output_type: output_type.clone(),
                            input_types: to.inputs.clone(),
                        });
                    }
                }
            }
        }
        Ok(ordered)
    }

    pub fn run(
        &self,
        runtime: &Arcade,
        external: Vec<ToolValue>,
        cancelled: &AtomicBool,
    ) -> Result<HashMap<String, Vec<ToolValue>>, PipelineError> {
        let order = self.validate(runtime)?;
        let mut results: HashMap<String, Vec<ToolValue>> = HashMap::new();
        for node in order {
            if cancelled.load(Ordering::Relaxed) {
                return Err(PipelineError::Cancelled);
            }
            let mut inputs = Vec::with_capacity(node.inputs.len());
            for source in &node.inputs {
                let input = match source {
                    InputSource::External { index } => external
                        .get(*index)
                        .cloned()
                        .ok_or(PipelineError::MissingExternal(*index))?,
                    InputSource::Node {
                        node_id,
                        output_index,
                    } => results
                        .get(node_id)
                        .and_then(|outputs| outputs.get(*output_index))
                        .cloned()
                        .ok_or_else(|| PipelineError::MissingOutput(node_id.clone()))?,
                };
                inputs.push(input);
            }
            let result = runtime
                .run_tool_with_cancel(
                    ToolRequest {
                        tool_id: node.tool_id.clone(),
                        inputs,
                        options: node.options.clone(),
                    },
                    cancelled,
                )
                .map_err(|error| PipelineError::Execution(node.id.clone(), error))?;
            if result.status == ResultStatus::Error {
                return Err(PipelineError::StageFailed(
                    node.id.clone(),
                    result.message.unwrap_or_else(|| "unknown error".into()),
                ));
            }
            results.insert(node.id.clone(), result.outputs);
        }
        Ok(results)
    }
}

fn visit<'a>(
    node: &'a PipelineNode,
    nodes: &HashMap<&str, &'a PipelineNode>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
    ordered: &mut Vec<&'a PipelineNode>,
) -> Result<(), PipelineError> {
    if visited.contains(node.id.as_str()) {
        return Ok(());
    }
    if !visiting.insert(node.id.as_str()) {
        return Err(PipelineError::Cycle);
    }
    for input in &node.inputs {
        if let InputSource::Node { node_id, .. } = input {
            let dependency = nodes
                .get(node_id.as_str())
                .ok_or_else(|| PipelineError::UnknownStage(node_id.clone()))?;
            visit(dependency, nodes, visiting, visited, ordered)?;
        }
    }
    visiting.remove(node.id.as_str());
    visited.insert(node.id.as_str());
    ordered.push(node);
    Ok(())
}

fn compatible(output_type: &str, input_type: &str) -> bool {
    output_type == input_type || input_type.strip_suffix("[]") == Some(output_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_cycles_and_bad_types() {
        let runtime = Arcade::in_memory().unwrap();
        let mut pipeline = Pipeline {
            id: "test".into(),
            name: "Test Pipeline".into(),
            version: 1,
            nodes: vec![
                PipelineNode {
                    id: "a".into(),
                    tool_id: "arcade.text.case".into(),
                    inputs: vec![InputSource::Node {
                        node_id: "b".into(),
                        output_index: 0,
                    }],
                    options: Value::Null,
                },
                PipelineNode {
                    id: "b".into(),
                    tool_id: "arcade.text.clean".into(),
                    inputs: vec![InputSource::Node {
                        node_id: "a".into(),
                        output_index: 0,
                    }],
                    options: Value::Null,
                },
            ],
            output_nodes: vec!["b".into()],
        };
        assert!(matches!(
            pipeline.validate(&runtime),
            Err(PipelineError::Cycle)
        ));
        pipeline.nodes[0].inputs = vec![InputSource::External { index: 0 }];
        assert_eq!(pipeline.validate(&runtime).unwrap().len(), 2);
    }

    #[test]
    fn executes_typed_chain() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "clean".into(),
            name: "Clean Text".into(),
            version: 1,
            nodes: vec![
                PipelineNode {
                    id: "upper".into(),
                    tool_id: "arcade.text.case".into(),
                    inputs: vec![InputSource::External { index: 0 }],
                    options: serde_json::json!({"mode":"upper"}),
                },
                PipelineNode {
                    id: "trim".into(),
                    tool_id: "arcade.text.clean".into(),
                    inputs: vec![InputSource::Node {
                        node_id: "upper".into(),
                        output_index: 0,
                    }],
                    options: serde_json::json!({"trim":true}),
                },
            ],
            output_nodes: vec!["trim".into()],
        };
        let results = pipeline
            .run(
                &runtime,
                vec![ToolValue::text(" hello ", "text/plain")],
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(results["trim"][0].value, "HELLO");
    }

    #[test]
    fn validation_rejects_stages_without_required_inputs() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "missing-input".into(),
            name: "Missing Input".into(),
            version: 1,
            nodes: vec![PipelineNode {
                id: "case".into(),
                tool_id: "arcade.text.case".into(),
                inputs: vec![],
                options: Value::Null,
            }],
            output_nodes: vec!["case".into()],
        };

        assert!(matches!(
            pipeline.validate(&runtime),
            Err(PipelineError::MissingInputs(stage)) if stage == "case"
        ));
    }

    #[test]
    fn validation_allows_stages_with_optional_inputs_to_receive_none() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "optional-input".into(),
            name: "Optional Input".into(),
            version: 1,
            nodes: vec![PipelineNode {
                id: "processes".into(),
                tool_id: "arcade.system.process".into(),
                inputs: vec![],
                options: Value::Null,
            }],
            output_nodes: vec!["processes".into()],
        };

        assert!(pipeline.validate(&runtime).is_ok());
    }

    #[test]
    fn validation_enforces_multi_input_minimums() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "two-inputs".into(),
            name: "Two Inputs".into(),
            version: 1,
            nodes: vec![PipelineNode {
                id: "merge".into(),
                tool_id: "arcade.pdf.merge".into(),
                inputs: vec![InputSource::External { index: 0 }],
                options: Value::Null,
            }],
            output_nodes: vec!["merge".into()],
        };

        assert!(matches!(
            pipeline.validate(&runtime),
            Err(PipelineError::TooFewInputs {
                stage,
                required: 2,
            }) if stage == "merge"
        ));
    }

    #[test]
    fn validation_rejects_pipelines_without_outputs() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "no-output".into(),
            name: "No Output".into(),
            version: 1,
            nodes: vec![PipelineNode {
                id: "case".into(),
                tool_id: "arcade.text.case".into(),
                inputs: vec![InputSource::External { index: 0 }],
                options: Value::Null,
            }],
            output_nodes: vec![],
        };

        assert!(matches!(
            pipeline.validate(&runtime),
            Err(PipelineError::NoOutputs)
        ));
    }

    #[test]
    fn validation_allows_inputless_generator_stages() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "system-info".into(),
            name: "System Info".into(),
            version: 1,
            nodes: vec![PipelineNode {
                id: "info".into(),
                tool_id: "arcade.system.system-info".into(),
                inputs: vec![],
                options: Value::Null,
            }],
            output_nodes: vec!["info".into()],
        };

        assert!(pipeline.validate(&runtime).is_ok());
    }
}
