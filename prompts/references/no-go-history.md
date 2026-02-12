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
