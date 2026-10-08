# Pipelines

Current implementation, checked against `crates/arcade-core/src/pipeline.rs`
on 2026-10-08. A pipeline is a saved, versioned, typed directed acyclic graph
(DAG). It runs through the same core as direct tool calls.

## Model and execution

A definition contains `id`, `name`, `version`, `nodes` and `outputNodes`.
Each node chooses either a local `toolId` or a `link` identity containing the
peer app, action and action version. Inputs reference an external input index
or a preceding node's output index. Options are saved with the node.

1. Validate identities, required inputs, output nodes, cycles and compatible
   types. Check external inputs and local engine availability before opening
   a peer UI. Nested saved pipelines are unsupported.
2. An interactive peer stage may appear only first. Other peer stages must be
   headless. A missing/disabled peer or a changed action version blocks execution
   with a repair error.
3. Collect effects. Network, device-send and command-execution effects require
   approval; the approval is bound to a hash of the definition and effects.
   A changed definition requires approval again. Password controls cannot be
   stored in pipeline options.
4. Execute nodes sequentially in topological order. Branching dependencies are
   valid, but independent branches do not run concurrently.
5. Pass scoped artifact references between stages. Copy peer handoff files into
   the pipeline workspace before their lifetime ends. Cancellation and stage
   errors stop execution and clean the scoped workspace; successful final
   outputs are preserved through the artifact handling code.

The editor supports saving, running and repairing workflows, including peer
stages. Pipelines are also exposed through `box.pipelines` and
`box.pipeline.run`; consumers store the pipeline ID in `options.pipeline`.

## Boundaries

There is no generic conditional-node language, reusable-variable system,
parallel scheduler or automatic retry policy. A node can use a tool's own
batch input support; this is not a general batch scheduler. Failed outbound
or destructive actions are not automatically retried. Output behavior remains
subject to each tool's scoped-file and never-overwrite rules.

See [Arcade Link](arcade-link.md), [storage](storage.md) and [status](STATUS.md).
