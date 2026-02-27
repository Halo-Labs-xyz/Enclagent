# Agentic Engineering Context

Status: Guidance
Last updated: 2026-02-27

## Purpose

Provide a professional framing for the recent workflow shift from manual implementation-first development to agent-orchestrated software delivery.

## Temporal Context

Across December 2025 through February 2026, coding-agent reliability materially improved in three areas:

- Long-horizon task persistence (fewer early drop-offs on multi-step work).
- Cross-step coherence (better continuity between setup, implementation, testing, and packaging).
- Recovery behavior (better autonomous handling of routine tool and environment failures).

Operational consequence: tasks that were previously multi-hour or multi-day implementation sessions can now be delegated as bounded execution runs when requirements and validation criteria are explicit.

## What Changed in Practice

| Dimension | Earlier pattern | Current pattern |
|---|---|---|
| Human effort center | Direct code production | Task design, orchestration, and verification |
| Work unit | File/feature implementation | Verifiable execution packet |
| Throughput strategy | Single-threaded personal execution | Parallel multi-agent execution with review gates |
| Quality control | Ad hoc manual checks | Deterministic validation commands and artifact review |
| Risk posture | Implicit trust in author process | Explicit trust via tests, receipts, and policy checks |

## Core Principle: Decompose at Verification Boundaries

The key practice is decomposition. A task is right-sized when one agent can complete it end-to-end and the result can be verified deterministically without broad interpretation.

Use these criteria for each task packet:

- One objective with unambiguous completion.
- Explicit inputs and required environment.
- Clear scope boundaries (files, modules, interfaces).
- Concrete constraints (security, performance, policy, runtime behavior).
- Required outputs (code, tests, docs, config, migration, evidence).
- Executable validation gates (exact commands and expected pass condition).

## Task Sizing Heuristic

| Sizing error | Symptom | Correction |
|---|---|---|
| Too large | Agent drifts, retries broadly, misses edge constraints | Split by subsystem or lifecycle stage |
| Too small | High orchestration overhead, slow total delivery | Merge adjacent deterministic steps |
| Under-specified | Correct-looking but non-compliant output | Add explicit constraints and acceptance tests |
| Over-constrained | Slow execution, brittle adaptation to minor failures | Keep only outcome-critical constraints |

## Standard Task Contract

Use this structure when assigning work:

```text
Objective:
Scope:
Out of scope:
Inputs:
Constraints:
Deliverables:
Validation commands:
Evidence required:
```

## Recursive Self-Improvement in Practice

Recursive self-improvement is a second-order effect where agent outputs improve the system that runs future agents.

Three layers are relevant:

- Run level: the agent recovers from failures during a single execution (diagnose, patch, retry, continue).
- System level: completed work improves future execution quality through better prompts, scripts, tests, and runbooks.
- Platform level: agents help build orchestration, evaluation, and policy tooling that increases future agent reliability.

Compounding gain appears when each completed task leaves behind reusable operational assets and tighter validation contracts.

## Constraint: Improvement Must Be Verification-Bound

Unconstrained recursive loops amplify defects as efficiently as they amplify productivity. Improvement loops are accepted only when they remain attached to deterministic gates:

- Required validation commands must pass for every loop iteration.
- Policy guardrails must remain enforced for sensitive actions.
- Intent, receipt, and verification artifacts must remain complete and auditable.
- Human merge authority remains final for risk acceptance.

This repository treats recursive self-improvement as a governed control loop, not autonomous authority expansion.

## Operating Model for Enclagent

This repository should treat agent execution as controlled production workflow, not autonomous trust:

1. Define scoped execution packets with deterministic validation.
2. Run agents in isolated branches/worktrees with non-overlapping ownership.
3. Enforce verification gates (lint, build, tests, policy checks).
4. Review diffs and operational artifacts, not only summaries.
5. Merge only when evidence matches stated acceptance criteria.

This model preserves Enclagent priorities: verifiability, security controls, and deterministic audit artifacts.

## Limits and Non-Goals

- Agentic workflows reduce implementation latency, not decision accountability.
- Ambiguous product direction, unstable requirements, and non-verifiable creative work still require heavier human iteration.
- Sensitive operations remain policy-gated; autonomy does not bypass signer, secrets, or guardrail requirements.
