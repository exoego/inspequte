# Rule Ideation No-Go History

This reference is used by `prompts/ideate-rule.md` to avoid proposing duplicate or low-value rule ideas.
Append one entry each time verify returns `No-Go`.

## Entry format
- `rule-id`: snake_case identifier
- `rule idea`: short summary used in ideation
- `no-go reason`: concise reason summary from verify
- `run-url`: GitHub Actions run URL for traceability

## Entries

### mutate_unmodifiable_collection
- rule-id: `mutate_unmodifiable_collection`
- rule idea: Detect attempts to mutate collections that are known to be unmodifiable because they were created by JDK unmodifiable factories in the same method.
- no-go reason: build and test failures from missing opcode constants; no implementation/tests in verify-input to validate spec requirements
- run-url: https://github.com/KengoTODA/inspequte/actions/runs/21924738785

## 2026-02-12T00:38:17Z | return_in_finally
- rule-id: `return_in_finally`
- rule idea: Detect return statements inside finally blocks that override exceptions or prior returns.  
- no-go reason: The implementation emits the spec message text verbatim via `result_message("Return in finally overrides exceptions or prior returns. Move the return outside the finally block or return after the try/finally.")` in `src/rules/return_in_finally/mod.rs` within the change set. This matches the spec Output message requirement. Evidence: `verify-input/diff.patch`.
- run-url: https://github.com/KengoTODA/inspequte/actions/runs/21928662084
